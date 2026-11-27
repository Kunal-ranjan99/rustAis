use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub tcp_bind_addr: String,
    pub http_bind_addr: String,
    pub redis_url: String,
    pub influx_host: String,
    pub influx_org: String,
    pub influx_bucket: String,
    pub influx_token: String,
    pub ebs_data_dir: Option<String>,
    pub max_tcp_connections: usize,
    pub tcp_idle_timeout_secs: u64,
    pub recent_buffer_size: usize,
}

impl Config {
    pub fn from_env() -> Self {
        let ebs_data_dir = env::var("EBS_DATA_DIR")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());

        Self {
            tcp_bind_addr: env::var("TCP_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:5001".to_string()),
            http_bind_addr: env::var("HTTP_BIND_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            redis_url: env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string()),
            influx_host: env::var("INFLUX_HOST")
                .unwrap_or_else(|_| "http://127.0.0.1:8086".to_string()),
            influx_org: env::var("INFLUX_ORG").unwrap_or_else(|_| "my-org".to_string()),
            influx_bucket: env::var("INFLUX_BUCKET").unwrap_or_else(|_| "vltd_data".to_string()),
            influx_token: env::var("INFLUX_TOKEN")
                .unwrap_or_else(|_| "my-super-secret-influx-token".to_string()),
            ebs_data_dir,
            max_tcp_connections: env::var("MAX_TCP_CONNECTIONS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5000),
            tcp_idle_timeout_secs: env::var("TCP_IDLE_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(90),
            recent_buffer_size: env::var("RECENT_BUFFER_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(20),
        }
    }
}
