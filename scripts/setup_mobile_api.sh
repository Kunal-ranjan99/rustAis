#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# Separate Setup & Permanent Startup Script for VLTD Mobile Node.js API
# Usage: bash ~/rustAis/scripts/setup_mobile_api.sh
# ==============================================================================

APP_DIR="${HOME}/rustAis"
API_DIR="${APP_DIR}/mobile-api"

echo "======================================================================"
echo " Setting up VLTD Mobile Node.js API (Port 3019)"
echo "======================================================================"

# 1. Install Node.js 20 LTS if missing
if ! command -v node >/dev/null 2>&1; then
  echo "==> [1/5] Installing Node.js 20 LTS..."
  curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
  sudo apt-get install -y nodejs
fi
node -v
npm -v

# 2. Install PM2 globally if missing
if ! command -v pm2 >/dev/null 2>&1; then
  echo "==> [2/5] Installing PM2 process manager..."
  sudo npm install -g pm2
fi

# 3. Pull latest repo code
echo "==> [3/5] Pulling latest code from GitHub..."
if [[ -d "${APP_DIR}/.git" ]]; then
  git -C "${APP_DIR}" fetch origin
  git -C "${APP_DIR}" pull --ff-only origin main
else
  git clone https://github.com/Kunal-ranjan99/rustAis.git "${APP_DIR}"
fi
chmod +x "${APP_DIR}/scripts/"*.sh

# 4. Install dependencies & prepare .env
echo "==> [4/5] Installing Node.js dependencies..."
cd "${API_DIR}"
npm install --omit=dev

if [[ ! -f "${API_DIR}/src/.env" ]]; then
  cp "${API_DIR}/.env.example" "${API_DIR}/src/.env"
  echo ""
  echo "  NOTE: Created ${API_DIR}/src/.env from template."
  echo "  Edit DB_USER, DB_PASSWORD, DB_DATABASE, JWT_SECRET in ${API_DIR}/src/.env before production use!"
fi

# 5. Start permanently via PM2
echo "==> [5/5] Starting vltd-mobile-api permanently with PM2..."
pm2 delete vltd-mobile-api >/dev/null 2>&1 || true
pm2 start ecosystem.config.js
pm2 save

# Enable PM2 auto-start on EC2 reboot
sudo env PATH="$PATH:$(dirname "$(which node)")" "$(which pm2)" startup systemd -u "${USER}" --hp "${HOME}" >/dev/null 2>&1 || true
pm2 save

sleep 2
echo ""
echo "======================================================================"
echo " Mobile API Running!"
echo "======================================================================"
curl -s http://127.0.0.1:3019/health || true
echo ""
echo ""
echo "  Mobile API URL  : http://<YOUR_EC2_PUBLIC_IP>:3019"
echo "  Edit Env Config : nano ~/rustAis/mobile-api/src/.env"
echo "  Live Logs       : pm2 logs vltd-mobile-api"
echo "  Future Updates  : bash ~/rustAis/scripts/update_mobile_api.sh"
echo "======================================================================"
