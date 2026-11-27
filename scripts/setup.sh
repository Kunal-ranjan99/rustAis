#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# Single-Command Ubuntu EC2 Setup & Permanent Daemon Deployment for rustAis
# Usage:
#   curl -sSL https://raw.githubusercontent.com/Kunal-ranjan99/rustAis/main/scripts/setup.sh | bash
# Optional EBS volume auto-mount:
#   curl -sSL https://raw.githubusercontent.com/Kunal-ranjan99/rustAis/main/scripts/setup.sh | EBS_DEVICE=/dev/nvme1n1 bash
# ==============================================================================

REPO_URL="https://github.com/Kunal-ranjan99/rustAis.git"
APP_USER="${USER:-ubuntu}"
APP_DIR="${HOME}/rustAis"
INFLUX_DEB_URL="https://dl.influxdata.com/influxdb/releases/influxdb2_2.7.11-1_amd64.deb"
INFLUX_CLI_URL="https://dl.influxdata.com/influxdb/releases/influxdb2-client-2.7.5-linux-amd64.tar.gz"

echo "======================================================================"
echo " Provisioning VLTD AIS-140 Rust Gateway on Ubuntu EC2"
echo "======================================================================"

# 0. Ensure 2GB swap on low-RAM EC2 instances (e.g. t2.micro / t3.micro) so rustup & cargo never OOM
TOTAL_MEM_MB=$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo)
if [[ "$TOTAL_MEM_MB" -lt 3072 ]] && ! swapon --show | grep -q '/swapfile'; then
  echo "==> [0/7] Detected ${TOTAL_MEM_MB}MB RAM; creating 2GB swapfile to prevent OOM during Rust build..."
  sudo fallocate -l 2G /swapfile || sudo dd if=/dev/zero of=/swapfile bs=1M count=2048
  sudo chmod 600 /swapfile
  sudo mkswap /swapfile
  sudo swapon /swapfile
  if ! grep -q '/swapfile' /etc/fstab; then
    echo '/swapfile none swap sw 0 0' | sudo tee -a /etc/fstab >/dev/null
  fi
fi

# 1. Base OS packages
echo "==> [1/7] Installing system dependencies..."
sudo apt-get update -y
sudo apt-get install -y curl wget git build-essential pkg-config libssl-dev ca-certificates

# 2. Rust toolchain (minimal profile = skips docs/clippy/rustfmt for low RAM & fast install)
if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi
if ! command -v cargo >/dev/null 2>&1 || ! rustc --version >/dev/null 2>&1; then
  echo "==> [2/7] Installing Rust toolchain (minimal profile)..."
  rm -rf "$HOME/.rustup" "$HOME/.cargo"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
# shellcheck disable=SC1091
source "$HOME/.cargo/env"
rustc --version
cargo --version

# 3. Redis (live cache)
echo "==> [3/7] Installing and configuring Redis..."
sudo apt-get install -y redis-server
sudo sysctl -w vm.overcommit_memory=1
echo "vm.overcommit_memory = 1" | sudo tee /etc/sysctl.d/99-redis-overcommit.conf >/dev/null
sudo systemctl enable redis-server
sudo systemctl restart redis-server

# 4. InfluxDB v2 + CLI
if ! command -v influxd >/dev/null 2>&1; then
  echo "==> [4/7] Installing InfluxDB v2 (2.7.11)..."
  curl -fSL "${INFLUX_DEB_URL}" -o /tmp/influxdb2.deb
  sudo dpkg -i /tmp/influxdb2.deb
  rm -f /tmp/influxdb2.deb
fi

if ! command -v influx >/dev/null 2>&1; then
  echo "==> Installing Influx CLI..."
  curl -fSL "${INFLUX_CLI_URL}" -o /tmp/influx-cli.tar.gz
  rm -rf /tmp/influx-cli-extract && mkdir -p /tmp/influx-cli-extract
  tar -xzf /tmp/influx-cli.tar.gz -C /tmp/influx-cli-extract
  sudo cp /tmp/influx-cli-extract/influx /usr/local/bin/influx
  sudo chmod +x /usr/local/bin/influx
  rm -rf /tmp/influx-cli-extract /tmp/influx-cli.tar.gz
fi

# 5. Clone or pull repository
echo "==> [5/7] Fetching repository into ${APP_DIR}..."
if [[ -d "${APP_DIR}/.git" ]]; then
  git -C "${APP_DIR}" fetch origin
  git -C "${APP_DIR}" pull --ff-only origin main
else
  git clone "${REPO_URL}" "${APP_DIR}"
fi
chmod +x "${APP_DIR}/scripts/"*.sh

# Optional EBS mount + InfluxDB EBS migration
if [[ -n "${EBS_DEVICE:-}" ]]; then
  echo "==> Configuring dedicated EBS volume ${EBS_DEVICE} for InfluxDB & raw archive..."
  sudo bash "${APP_DIR}/scripts/mount_ebs_influx.sh" "${EBS_DEVICE}"
else
  sudo mkdir -p /mnt/ebs/vltd_raw
  sudo chown -R "${APP_USER}:${APP_USER}" /mnt/ebs/vltd_raw
  sudo systemctl enable influxdb
  sudo systemctl start influxdb
fi

# Wait for InfluxDB HTTP API and initialize credentials idempotently
echo "==> Waiting for InfluxDB API..."
for _ in $(seq 1 20); do
  if curl -sf http://127.0.0.1:8086/health >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

influx setup \
  -n default \
  --username admin \
  --password "AdminPassword123!" \
  --org my-org \
  --bucket vltd_data \
  --token "my-super-secret-influx-token" \
  --force >/dev/null 2>&1 || echo "==> InfluxDB already initialized (keeping existing credentials)."

# 6. Build Rust release binary
echo "==> [6/7] Building optimized Rust release binary..."
cd "${APP_DIR}"
if [[ ! -f "${APP_DIR}/.env" ]]; then
  cp "${APP_DIR}/.env.example" "${APP_DIR}/.env"
fi
if [[ "$TOTAL_MEM_MB" -lt 3072 ]]; then
  CARGO_BUILD_JOBS=1 cargo build --release
else
  cargo build --release
fi

# 7. Install permanent systemd daemon
echo "==> [7/7] Configuring systemd service (vltd-gateway)..."
cat << EOF | sudo tee /etc/systemd/system/vltd-gateway.service >/dev/null
[Unit]
Description=VLTD AIS-140 Rust TCP Gateway and WebGL Dashboard
After=network-online.target redis-server.service influxdb.service
Wants=network-online.target redis-server.service influxdb.service

[Service]
Type=simple
User=${APP_USER}
WorkingDirectory=${APP_DIR}
EnvironmentFile=${APP_DIR}/.env
ExecStart=${APP_DIR}/target/release/vltd_gateway
Restart=always
RestartSec=3s
StandardOutput=journal
StandardError=journal
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable vltd-gateway
sudo systemctl restart vltd-gateway

sleep 2
echo ""
echo "======================================================================"
echo " Deployment Complete!"
echo "======================================================================"
curl -s http://127.0.0.1:8080/health || true
echo ""
echo ""
echo "  TCP AIS-140 Port : 5001"
echo "  WebGL Dashboard  : http://<YOUR_EC2_PUBLIC_IP>:8080"
echo "  Live Logs        : sudo journalctl -u vltd-gateway -f"
echo "  Future Updates   : bash ~/rustAis/scripts/update.sh"
echo "======================================================================"
