#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# Single-Command Update & Restart Script for VLTD AIS-140 Rust Gateway
# Usage: bash ~/rustAis/scripts/update.sh
# ==============================================================================

APP_DIR="${HOME}/rustAis"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

echo "==> [1/3] Pulling latest code from GitHub..."
cd "$APP_DIR"
git fetch origin
git pull --ff-only origin main
chmod +x scripts/*.sh

echo "==> [2/3] Building optimized release binary (live server stays online during compile)..."
TOTAL_MEM_MB=$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo)
if [[ "$TOTAL_MEM_MB" -lt 3072 ]]; then
  CARGO_BUILD_JOBS=1 cargo build --release
else
  cargo build --release
fi

echo "==> [3/3] Restarting vltd-gateway systemd service..."
sudo systemctl restart vltd-gateway

sleep 2
echo ""
echo "==> Service Status:"
sudo systemctl is-active vltd-gateway
echo "==> Health Check:"
curl -s http://127.0.0.1:8080/health
echo ""
echo "==> Update complete! View logs: sudo journalctl -u vltd-gateway -f"
