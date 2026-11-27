#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# Single-Command Ubuntu EC2 Setup & Permanent Daemon Deployment for rustAis
# Usage:
#   curl -sSL https://raw.githubusercontent.com/Kunal-ranjan99/rustAis/main/scripts/setup.sh | bash
# Optional EBS volume auto-mount:
#   EBS_DEVICE=/dev/nvme1n1 curl -sSL https://raw.githubusercontent.com/Kunal-ranjan99/rustAis/main/scripts/setup.sh | bash
# ==============================================================================

REPO_URL="https://github.com/Kunal-ranjan99/rustAis.git"
APP_USER="${USER:-ubuntu}"
APP_DIR="${HOME}/rustAis"
INFLUX_VER="2.7.5"

echo "======================================================================"
echo " Provisioning VLTD AIS-140 Rust Gateway on Ubuntu EC2"
echo "======================================================================"

# 1. Base OS packages
echo "==> [1/7] Installing system dependencies..."
sudo apt-get update -y
sudo apt-get install -y curl wget git build-essential pkg-config libssl-dev ca-certificates

# 2. Rust toolchain
if ! command -v cargo >/dev/null 2>&1; then
  echo "==> [2/7] Installing Rust toolchain..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
fi
# shellcheck disable=SC1091
source "$HOME/.cargo/env"
rustc --version
cargo --version

# 3. Redis (live cache)
echo "==> [3/7] Installing and configuring Redis..."
sudo apt-get install -y redis-server
sudo sysctl -w vm.overcommit_memory=1
if ! grep -q "vm.overcommit_memory" /etc/sysctl.conf; then
  echo "vm.overcommit_memory = 1" | sudo tee -a /etc/sysctl.conf >/dev/null
fi
sudo systemctl enable redis-server
sudo systemctl restart redis-server

# 4. InfluxDB v2 + CLI
if ! command -v influxd >/dev/null 2>&1; then
  echo "==> [4/7] Installing InfluxDB v2 (${INFLUX_VER})..."
  wget -q "https://dl.influxdata.com/influxdb/releases/influxdb2-${INFLUX_VER}-amd64.deb" -O /tmp/influxdb2.deb
  sudo dpkg -i /tmp/influxdb2.deb
  rm -f /tmp/influxdb2.deb
fi

if ! command -v influx >/dev/null 2>&1; then
  echo "==> Installing Influx CLI..."
  wget -q "https://dl.influxdata.com/influxdb/releases/influxdb2-client-${INFLUX_VER}-linux-amd64.tar.gz" -O /tmp/influx-cli.tar.gz
  tar -xzf /tmp/influx-cli.tar.gz -C /tmp
  sudo cp /tmp/influx /usr/local/bin/influx
  sudo chmod +x /usr/local/bin/influx
  rm -rf /tmp/influx /tmp/influx-cli.tar.gz /tmp/README.md /tmp/LICENSE
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
cargo build --release

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
