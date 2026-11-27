use crate::ais140::TelemetryRecord;
use crate::storage::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio_stream::{wrappers::BroadcastStream, Stream, StreamExt};

const DASHBOARD_HTML: &str = include_str!("../static/index.html");

#[derive(Deserialize)]
pub struct DeviceQuery {
    pub imei: Option<String>,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
}

#[derive(Deserialize)]
pub struct InfluxHistoryQuery {
    pub imei: String,
    pub start: i64,
    pub end: i64,
}

#[derive(Serialize)]
pub struct DevicePacket {
    pub imei: String,
    pub packet: Option<String>,
}

#[derive(Serialize)]
pub struct HistoryRecord {
    pub imei: String,
    pub raw_packet: String,
    pub time: String,
}

#[derive(Deserialize)]
pub struct SimulateRequest {
    pub raw_packet: Option<String>,
    pub imei: Option<String>,
    pub vehicle_reg: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub speed_kmh: Option<f64>,
    pub heading: Option<f64>,
    pub packet_status: Option<String>,
    pub packet_type: Option<String>,
    pub ignition: Option<bool>,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        // WebGL Telemetry Dashboard
        .route("/", get(serve_dashboard))
        // Health & Live Telemetry APIs
        .route("/health", get(health_check))
        .route("/api/recent", get(get_recent_packets))
        .route("/api/live", get(get_live_devices))
        .route("/api/search", get(search_vehicles))
        .route("/api/stream", get(sse_stream))
        .route("/api/simulate", post(simulate_tcp_packet))
        // Reference VLTD REST APIs (100% compatible)
        .route("/devices", get(get_all_devices))
        .route("/devices/latest", get(get_devices_latest))
        .route("/devices/:imei", get(get_single_device_by_path))
        .route("/influx/history", get(query_influx_history))
        .with_state(state)
}

async fn serve_dashboard() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

async fn health_check(State(state): State<AppState>) -> impl IntoResponse {
    let stats = state.stats_snapshot().await;
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "service": "vltd_gateway",
            "tcp_addr": state.config.tcp_bind_addr,
            "stats": stats,
        })),
    )
}

/// Returns the latest 20 incoming AIS-140 packets (newest first).
async fn get_recent_packets(State(state): State<AppState>) -> impl IntoResponse {
    let buf = state.recent_packets.read().await;
    let list: Vec<TelemetryRecord> = buf.iter().cloned().collect();
    (StatusCode::OK, Json(list))
}

/// Returns the latest parsed telemetry record for every active device in memory.
async fn get_live_devices(State(state): State<AppState>) -> impl IntoResponse {
    let map = state.live_devices.read().await;
    let list: Vec<TelemetryRecord> = map.values().cloned().collect();
    (StatusCode::OK, Json(list))
}

/// Searches active devices and recent packets by IMEI or Vehicle Registration Number.
async fn search_vehicles(
    Query(params): Query<SearchQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let q = params.q.unwrap_or_default().trim().to_lowercase();
    let live_map = state.live_devices.read().await;
    let recent_buf = state.recent_packets.read().await;

    let matches_query = |r: &TelemetryRecord| -> bool {
        if q.is_empty() {
            return true;
        }
        r.imei.to_lowercase().contains(&q)
            || r.vehicle_reg
                .as_deref()
                .map(|v| v.to_lowercase().contains(&q))
                .unwrap_or(false)
            || r.raw_packet.to_lowercase().contains(&q)
    };

    let devices: Vec<TelemetryRecord> = live_map
        .values()
        .filter(|r| matches_query(r))
        .cloned()
        .collect();
    let packets: Vec<TelemetryRecord> = recent_buf
        .iter()
        .filter(|r| matches_query(r))
        .cloned()
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "query": q,
            "devices": devices,
            "packets": packets,
        })),
    )
}

/// Server-Sent Events stream pushing every incoming packet live to the WebGL dashboard.
async fn sse_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.broadcast_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|msg| match msg {
        Ok(record) => serde_json::to_string(&record)
            .ok()
            .map(|json| Ok(Event::default().event("telemetry").data(json))),
        Err(_) => None,
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

/// Sends a real AIS-140 `$PVT` packet over TCP to the local ingestion port (5001)
/// so the WebGL simulator exercises the full TCP -> Validator -> Redis/Influx/EBS -> SSE pipeline.
async fn simulate_tcp_packet(
    State(state): State<AppState>,
    Json(req): Json<SimulateRequest>,
) -> impl IntoResponse {
    let packet = if let Some(raw) = req.raw_packet.filter(|s| !s.trim().is_empty()) {
        raw.trim().to_string()
    } else {
        let now = Utc::now();
        let date = now.format("%d%m%Y").to_string();
        let time = now.format("%H%M%S").to_string();
        let imei = req.imei.unwrap_or_else(|| "864251050012345".to_string());
        let reg = req.vehicle_reg.unwrap_or_else(|| "DL01AB1234".to_string());
        let status = req.packet_status.unwrap_or_else(|| "L".to_string());
        let ptype = req.packet_type.unwrap_or_else(|| "NR".to_string());
        let lat = req.lat.unwrap_or(28.6139);
        let lon = req.lon.unwrap_or(77.2090);
        let lat_dir = if lat >= 0.0 { "N" } else { "S" };
        let lon_dir = if lon >= 0.0 { "E" } else { "W" };
        let speed = req.speed_kmh.unwrap_or(42.5);
        let heading = req.heading.unwrap_or(90.0);
        let ign = if req.ignition.unwrap_or(true) { "1" } else { "0" };

        let body = format!(
            "$PVT,AIS140,1.0,{},01,{},{},{},1,{},{},{:.6},{},{:.6},{},{:.1},{:.1},11,215.0,0.9,0.8,AIRTEL,{},1,13.8,4.1,0",
            ptype,
            status,
            imei,
            reg,
            date,
            time,
            lat.abs(),
            lat_dir,
            lon.abs(),
            lon_dir,
            speed,
            heading,
            ign
        );
        let payload = body.strip_prefix('$').unwrap_or(&body);
        let checksum = payload.bytes().fold(0u8, |acc, b| acc ^ b);
        format!("{}*{:02X}", body, checksum)
    };

    let port = state
        .config
        .tcp_bind_addr
        .rsplit(':')
        .next()
        .unwrap_or("5001");
    let target = format!("127.0.0.1:{}", port);

    match TcpStream::connect(&target).await {
        Ok(mut stream) => {
            let line = format!("{}\n", packet);
            if let Err(e) = stream.write_all(line.as_bytes()).await {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("TCP write failed: {}", e) })),
                );
            }
            let _ = stream.shutdown().await;
            (
                StatusCode::OK,
                Json(serde_json::json!({ "status": "sent", "packet": packet })),
            )
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("TCP connect to {} failed: {}", target, e) })),
        ),
    }
}

// ---------------- REFERENCE VLTD REST API CONTROLLERS ----------------

async fn get_all_devices(State(state): State<AppState>) -> impl IntoResponse {
    let Some(mut con) = state.get_redis_conn().await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "Redis unavailable"})),
        )
            .into_response();
    };

    let imeis: Vec<String> = con.smembers("devices:active").await.unwrap_or_default();
    if imeis.is_empty() {
        return (StatusCode::OK, Json(Vec::<DevicePacket>::new())).into_response();
    }
    fetch_imeis_from_redis(&state, imeis).await
}

async fn get_devices_latest(
    Query(params): Query<DeviceQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let imeis: Vec<String> = match params.imei {
        Some(ref list) if !list.trim().is_empty() => list
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => {
            let Some(mut con) = state.get_redis_conn().await else {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({"error": "Redis unavailable"})),
                )
                    .into_response();
            };
            con.smembers("devices:active").await.unwrap_or_default()
        }
    };

    if imeis.is_empty() {
        return (StatusCode::OK, Json(Vec::<DevicePacket>::new())).into_response();
    }
    fetch_imeis_from_redis(&state, imeis).await
}

async fn get_single_device_by_path(
    Path(imei): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    fetch_imeis_from_redis(&state, vec![imei]).await
}

async fn fetch_imeis_from_redis(state: &AppState, imeis: Vec<String>) -> Response {
    let Some(mut con) = state.get_redis_conn().await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "Redis unavailable"})),
        )
            .into_response();
    };

    let keys: Vec<String> = imeis.iter().map(|id| format!("packet:pvt:{}", id)).collect();
    let values: Vec<Option<String>> = con
        .mget(&keys)
        .await
        .unwrap_or_else(|_| vec![None; keys.len()]);

    let results: Vec<DevicePacket> = imeis
        .into_iter()
        .zip(values.into_iter())
        .map(|(imei, packet)| DevicePacket { imei, packet })
        .collect();

    (StatusCode::OK, Json(results)).into_response()
}

async fn query_influx_history(
    Query(params): Query<InfluxHistoryQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // Sanitize IMEI to prevent Flux injection
    let safe_imei: String = params
        .imei
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();

    let stop_clause = if params.end <= 0 {
        "now()".to_string()
    } else {
        format!("{}", params.end)
    };

    let flux_query = format!(
        r#"from(bucket: "{}")
  |> range(start: {}, stop: {})
  |> filter(fn: (r) => r["_measurement"] == "vltd_packet")
  |> filter(fn: (r) => r["imei"] == "{}")
  |> filter(fn: (r) => r["_field"] == "raw")"#,
        state.config.influx_bucket, params.start, stop_clause, safe_imei
    );

    let url = format!(
        "{}/api/v2/query?org={}",
        state.config.influx_host, state.config.influx_org
    );
    let body_json = serde_json::json!({
        "query": flux_query,
        "type": "flux"
    });

    let res = match state
        .http_client
        .post(&url)
        .header("Authorization", format!("Token {}", state.config.influx_token))
        .header("Accept", "application/csv")
        .json(&body_json)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    let csv_text = match res.text().await {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    let mut records: Vec<HistoryRecord> = Vec::new();
    for line in csv_text.lines() {
        if line.starts_with('#') || line.starts_with(",result") || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() >= 8 {
            let time_str = parts[5].trim().to_string();
            let raw_val = parts[6..parts.len() - 3].join(",").trim().to_string();
            records.push(HistoryRecord {
                imei: safe_imei.clone(),
                raw_packet: if raw_val.is_empty() {
                    parts[6].to_string()
                } else {
                    raw_val
                },
                time: time_str,
            });
        }
    }

    (StatusCode::OK, Json(records)).into_response()
}
