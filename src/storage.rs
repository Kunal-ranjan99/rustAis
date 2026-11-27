use crate::ais140::TelemetryRecord;
use crate::config::Config;
use chrono::Utc;
use redis::AsyncCommands;
use reqwest::Client as HttpClient;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, RwLock};
use tracing::{error, info, warn};

pub struct ServerStats {
    pub started_at: String,
    pub active_connections: AtomicUsize,
    pub total_packets: AtomicU64,
    pub valid_packets: AtomicU64,
    pub invalid_packets: AtomicU64,
    pub influx_writes_ok: AtomicU64,
    pub influx_writes_err: AtomicU64,
    pub ebs_writes_ok: AtomicU64,
    pub ebs_writes_err: AtomicU64,
}

impl ServerStats {
    pub fn new() -> Self {
        Self {
            started_at: Utc::now().to_rfc3339(),
            active_connections: AtomicUsize::new(0),
            total_packets: AtomicU64::new(0),
            valid_packets: AtomicU64::new(0),
            invalid_packets: AtomicU64::new(0),
            influx_writes_ok: AtomicU64::new(0),
            influx_writes_err: AtomicU64::new(0),
            ebs_writes_ok: AtomicU64::new(0),
            ebs_writes_err: AtomicU64::new(0),
        }
    }
}

#[derive(Serialize)]
pub struct StatsSnapshot {
    pub started_at: String,
    pub active_connections: usize,
    pub total_packets: u64,
    pub valid_packets: u64,
    pub invalid_packets: u64,
    pub influx_writes_ok: u64,
    pub influx_writes_err: u64,
    pub last_influx_error: Option<String>,
    pub ebs_writes_ok: u64,
    pub ebs_writes_err: u64,
    pub tracked_devices: usize,
    pub ebs_enabled: bool,
    pub ebs_data_dir: Option<String>,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub redis_client: redis::Client,
    pub redis_conn: Arc<RwLock<Option<redis::aio::ConnectionManager>>>,
    pub http_client: HttpClient,
    pub recent_packets: Arc<RwLock<VecDeque<TelemetryRecord>>>,
    pub live_devices: Arc<RwLock<HashMap<String, TelemetryRecord>>>,
    pub last_influx_error: Arc<RwLock<Option<String>>>,
    pub broadcast_tx: broadcast::Sender<TelemetryRecord>,
    pub stats: Arc<ServerStats>,
    pub packet_seq: Arc<AtomicU64>,
}

impl AppState {
    pub async fn new(config: Config) -> Result<Self, Box<dyn std::error::Error>> {
        let redis_client = redis::Client::open(config.redis_url.as_str())?;
        let redis_conn = match redis::aio::ConnectionManager::new(redis_client.clone()).await {
            Ok(mut mgr) => {
                let pong: redis::RedisResult<String> =
                    redis::cmd("PING").query_async(&mut mgr).await;
                match pong {
                    Ok(res) => info!("Redis connected successfully: PING -> {}", res),
                    Err(e) => warn!("Redis ping failed (will retry automatically): {:?}", e),
                }
                Some(mgr)
            }
            Err(e) => {
                warn!("Redis unavailable at startup (will retry lazily): {:?}", e);
                None
            }
        };

        if let Some(ref dir) = config.ebs_data_dir {
            if let Err(e) = tokio::fs::create_dir_all(dir).await {
                warn!("Could not create EBS data dir '{}': {:?}", dir, e);
            } else {
                info!("Direct EBS archive enabled at: {}", dir);
            }
        }

        let http_client = HttpClient::builder()
            .timeout(Duration::from_secs(5))
            .pool_max_idle_per_host(16)
            .build()?;

        let (broadcast_tx, _) = broadcast::channel(256);

        Ok(Self {
            recent_packets: Arc::new(RwLock::new(VecDeque::with_capacity(
                config.recent_buffer_size + 1,
            ))),
            live_devices: Arc::new(RwLock::new(HashMap::new())),
            last_influx_error: Arc::new(RwLock::new(None)),
            config,
            redis_client,
            redis_conn: Arc::new(RwLock::new(redis_conn)),
            http_client,
            broadcast_tx,
            stats: Arc::new(ServerStats::new()),
            packet_seq: Arc::new(AtomicU64::new(1)),
        })
    }

    pub fn next_packet_id(&self) -> u64 {
        self.packet_seq.fetch_add(1, Ordering::Relaxed)
    }

    pub async fn stats_snapshot(&self) -> StatsSnapshot {
        let tracked_devices = self.live_devices.read().await.len();
        let last_influx_error = self.last_influx_error.read().await.clone();
        StatsSnapshot {
            started_at: self.stats.started_at.clone(),
            active_connections: self.stats.active_connections.load(Ordering::Relaxed),
            total_packets: self.stats.total_packets.load(Ordering::Relaxed),
            valid_packets: self.stats.valid_packets.load(Ordering::Relaxed),
            invalid_packets: self.stats.invalid_packets.load(Ordering::Relaxed),
            influx_writes_ok: self.stats.influx_writes_ok.load(Ordering::Relaxed),
            influx_writes_err: self.stats.influx_writes_err.load(Ordering::Relaxed),
            last_influx_error,
            ebs_writes_ok: self.stats.ebs_writes_ok.load(Ordering::Relaxed),
            ebs_writes_err: self.stats.ebs_writes_err.load(Ordering::Relaxed),
            tracked_devices,
            ebs_enabled: self.config.ebs_data_dir.is_some(),
            ebs_data_dir: self.config.ebs_data_dir.clone(),
        }
    }

    /// Records the incoming packet in memory (latest 20 ring buffer + live device map),
    /// broadcasts to WebGL SSE clients, and persists asynchronously to Redis, InfluxDB, and EBS.
    pub async fn ingest_record(&self, record: TelemetryRecord) {
        self.stats.total_packets.fetch_add(1, Ordering::Relaxed);
        if record.valid {
            self.stats.valid_packets.fetch_add(1, Ordering::Relaxed);
        } else {
            self.stats.invalid_packets.fetch_add(1, Ordering::Relaxed);
        }

        // 1. Update in-memory ring buffer (newest first, capped at recent_buffer_size = 20)
        {
            let mut buf = self.recent_packets.write().await;
            buf.push_front(record.clone());
            while buf.len() > self.config.recent_buffer_size {
                buf.pop_back();
            }
        }

        // 2. Update live device map for valid Live ('L') packets
        if record.valid && record.packet_status == "L" {
            let mut map = self.live_devices.write().await;
            map.insert(record.imei.clone(), record.clone());
        }

        // 3. Push to SSE subscribers (ignore error when zero browsers are connected)
        let _ = self.broadcast_tx.send(record.clone());

        // 4. Persist valid packets to Redis, InfluxDB, and EBS off the hot TCP read path
        if record.valid {
            let state = self.clone();
            tokio::spawn(async move {
                let redis_fut = async {
                    if record.packet_status == "L" {
                        state.save_to_redis(&record.imei, &record.raw_packet).await;
                    }
                };
                let influx_fut = state.save_to_influx(&record);
                let ebs_fut = state.append_to_ebs(&record);

                tokio::join!(redis_fut, influx_fut, ebs_fut);
            });
        }
    }

    pub async fn get_redis_conn(&self) -> Option<redis::aio::ConnectionManager> {
        {
            let guard = self.redis_conn.read().await;
            if let Some(ref mgr) = *guard {
                return Some(mgr.clone());
            }
        }
        // Lazy reconnect attempt
        match redis::aio::ConnectionManager::new(self.redis_client.clone()).await {
            Ok(mgr) => {
                let mut guard = self.redis_conn.write().await;
                *guard = Some(mgr.clone());
                Some(mgr)
            }
            Err(_) => None,
        }
    }

    async fn save_to_redis(&self, imei: &str, raw_packet: &str) {
        let Some(mut con) = self.get_redis_conn().await else {
            warn!("Redis unavailable; skipped live state update for {}", imei);
            return;
        };

        let key = format!("packet:pvt:{}", imei);
        if let Err(e) = con.set::<_, _, ()>(&key, raw_packet).await {
            error!("Redis SET error for {}: {:?}", imei, e);
        }
        if let Err(e) = con.sadd::<_, _, ()>("devices:active", imei).await {
            error!("Redis SADD error for {}: {:?}", imei, e);
        }
    }

    async fn save_to_influx(&self, record: &TelemetryRecord) {
        let now_nanos = Utc::now().timestamp_nanos_opt().unwrap_or(0);
        let escaped_packet = record
            .raw_packet
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', " ")
            .replace('\r', "");

        let mut fields = vec![format!("raw=\"{}\"", escaped_packet)];
        if let Some(lat) = record.lat {
            fields.push(format!("lat={}", lat));
        }
        if let Some(lon) = record.lon {
            fields.push(format!("lon={}", lon));
        }
        if let Some(speed) = record.speed_kmh {
            fields.push(format!("speed={}", speed));
        }
        if let Some(heading) = record.heading {
            fields.push(format!("heading={}", heading));
        }
        if let Some(ign) = record.ignition {
            fields.push(format!("ignition={}", if ign { 1 } else { 0 }));
        }

        let line = format!(
            "vltd_packet,imei={},type={} {} {}",
            record.imei,
            record.packet_status,
            fields.join(","),
            now_nanos
        );

        let url = format!(
            "{}/api/v2/write?org={}&bucket={}&precision=ns",
            self.config.influx_host, self.config.influx_org, self.config.influx_bucket
        );

        match self
            .http_client
            .post(&url)
            .header("Authorization", format!("Token {}", self.config.influx_token))
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(line)
            .send()
            .await
        {
            Ok(res) => {
                let status = res.status();
                if status.is_success() || status == reqwest::StatusCode::NO_CONTENT {
                    self.stats.influx_writes_ok.fetch_add(1, Ordering::Relaxed);
                    let mut guard = self.last_influx_error.write().await;
                    if guard.is_some() {
                        *guard = None;
                    }
                } else {
                    self.stats.influx_writes_err.fetch_add(1, Ordering::Relaxed);
                    let err = res.text().await.unwrap_or_default();
                    let msg = format!("HTTP {}: {}", status, err);
                    *self.last_influx_error.write().await = Some(msg.clone());
                    error!("InfluxDB {}", msg);
                }
            }
            Err(e) => {
                self.stats.influx_writes_err.fetch_add(1, Ordering::Relaxed);
                let msg = format!("Request failed: {}", e);
                *self.last_influx_error.write().await = Some(msg.clone());
                error!("InfluxDB {}", msg);
            }
        }
    }

    async fn append_to_ebs(&self, record: &TelemetryRecord) {
        let Some(ref dir) = self.config.ebs_data_dir else {
            return;
        };

        let day = Utc::now().format("%Y-%m-%d").to_string();
        let mut path = PathBuf::from(dir);
        path.push(format!("ais140-{}.ndjson", day));

        let Ok(mut json_line) = serde_json::to_string(record) else {
            return;
        };
        json_line.push('\n');

        match OpenOptions::new().create(true).append(true).open(&path).await {
            Ok(mut file) => {
                if let Err(e) = file.write_all(json_line.as_bytes()).await {
                    self.stats.ebs_writes_err.fetch_add(1, Ordering::Relaxed);
                    error!("EBS write error ({:?}): {:?}", path, e);
                } else {
                    self.stats.ebs_writes_ok.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(e) => {
                self.stats.ebs_writes_err.fetch_add(1, Ordering::Relaxed);
                error!("EBS open error ({:?}): {:?}", path, e);
            }
        }
    }
}
