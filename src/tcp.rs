use crate::ais140::{parse_ais140_packet, try_hex_to_string};
use crate::storage::AppState;
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tracing::{error, info, warn};

const MAX_ACCUMULATOR_BYTES: usize = 65_536; // 64 KB cap per connection to prevent OOM

pub async fn start_tcp_listener(state: AppState) {
    let addr = state.config.tcp_bind_addr.clone();
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("FATAL: Failed to bind TCP listener to {}: {:?}", addr, e);
            return;
        }
    };

    let semaphore = Arc::new(Semaphore::new(state.config.max_tcp_connections));
    info!(
        "VLTD AIS-140 TCP Gateway active on {} (max_conns={}, idle_timeout={}s)",
        addr, state.config.max_tcp_connections, state.config.tcp_idle_timeout_secs
    );

    loop {
        match listener.accept().await {
            Ok((socket, peer_addr)) => {
                let permit = match semaphore.clone().try_acquire_owned() {
                    Ok(p) => p,
                    Err(_) => {
                        warn!(
                            "Connection limit ({}) reached; dropping {}",
                            state.config.max_tcp_connections, peer_addr
                        );
                        drop(socket);
                        continue;
                    }
                };

                // Disable Nagle's algorithm for low-latency telemetry ingestion
                let _ = socket.set_nodelay(true);

                let state_clone = state.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    state_clone
                        .stats
                        .active_connections
                        .fetch_add(1, Ordering::Relaxed);
                    handle_device_socket(socket, peer_addr, &state_clone).await;
                    state_clone
                        .stats
                        .active_connections
                        .fetch_sub(1, Ordering::Relaxed);
                });
            }
            Err(e) => {
                warn!("TCP accept error: {:?}", e);
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
}

async fn handle_device_socket(mut socket: TcpStream, peer_addr: SocketAddr, state: &AppState) {
    info!("[CONNECT] {}", peer_addr);
    let mut accumulator: Vec<u8> = Vec::with_capacity(4096);
    let mut chunk = [0u8; 2048];
    let idle_duration = Duration::from_secs(state.config.tcp_idle_timeout_secs);

    loop {
        match timeout(idle_duration, socket.read(&mut chunk)).await {
            Ok(Ok(0)) => {
                info!("[DISCONNECT] {}", peer_addr);
                break;
            }
            Ok(Ok(n)) => {
                if accumulator.len() + n > MAX_ACCUMULATOR_BYTES {
                    warn!(
                        "[BUFFER OVERFLOW] {} exceeded {} bytes without valid delimiter; clearing buffer",
                        peer_addr, MAX_ACCUMULATOR_BYTES
                    );
                    accumulator.clear();
                }
                accumulator.extend_from_slice(&chunk[..n]);
                process_accumulator(&mut accumulator, state, &peer_addr).await;
            }
            Ok(Err(e)) => {
                warn!("[READ ERROR] {}: {:?}", peer_addr, e);
                break;
            }
            Err(_) => {
                info!(
                    "[IDLE TIMEOUT] {}s idle from {}; closing connection",
                    state.config.tcp_idle_timeout_secs, peer_addr
                );
                break;
            }
        }
    }
}

async fn process_accumulator(acc: &mut Vec<u8>, state: &AppState, peer_addr: &SocketAddr) {
    let text = String::from_utf8_lossy(acc).to_string();
    let resolved_text = if let Some(decoded) = try_hex_to_string(&text) {
        decoded
    } else {
        text
    };

    let mut remaining = resolved_text.as_str();
    let peer_str = peer_addr.to_string();

    while let Some(start_idx) = remaining.find("$PVT") {
        let after_start = &remaining[start_idx..];

        // Determine packet end:
        // 1. Delimited by `*` + checksum (supports 2-char NMEA `*HH` or 4-char `*XXXX` up to newline/end)
        // 2. Delimited by `\n` or `\r`
        let packet_end = if let Some(nl_idx) = after_start.find('\n') {
            Some(nl_idx + 1)
        } else if let Some(star_idx) = after_start.find('*') {
            let after_star = &after_start[star_idx + 1..];
            // If another $PVT starts right after checksum, stop there
            if let Some(next_pvt) = after_star.find("$PVT") {
                Some(star_idx + 1 + next_pvt)
            } else if after_star.len() >= 2 {
                // Consume 2-char hex checksum (or up to 4 hex chars if present)
                let hex_len = after_star
                    .chars()
                    .take(4)
                    .take_while(|c| c.is_ascii_hexdigit())
                    .count();
                if hex_len >= 2 {
                    Some(star_idx + 1 + hex_len)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        if let Some(end_idx) = packet_end {
            let packet = after_start[..end_idx].trim();
            remaining = &after_start[end_idx..];

            if !packet.is_empty() {
                let id = state.next_packet_id();
                let record = parse_ais140_packet(id, &peer_str, packet);
                if record.valid {
                    info!(
                        "[PKT #{}] IMEI={} status={} lat={:?} lon={:?} spd={:?}",
                        record.id,
                        record.imei,
                        record.packet_status,
                        record.lat,
                        record.lon,
                        record.speed_kmh
                    );
                } else {
                    warn!(
                        "[INVALID PKT #{}] from {}: {:?} | raw={}",
                        record.id, peer_str, record.validation_error, packet
                    );
                }
                state.ingest_record(record).await;
            }
        } else {
            // Incomplete packet — keep from `$PVT` onwards for the next TCP chunk
            remaining = after_start;
            break;
        }
    }

    acc.clear();
    acc.extend_from_slice(remaining.as_bytes());
}
