#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# Single-Command Update & Restart Script for VLTD Mobile Node.js API
# Usage: bash ~/rustAis/scripts/update_mobile_api.sh
# ==============================================================================

APP_DIR="${HOME}/rustAis"
API_DIR="${APP_DIR}/mobile-api"

echo "==> [1/3] Pulling latest code from GitHub..."
cd "${APP_DIR}"
git fetch origin
git pull --ff-only origin main
chmod +x scripts/*.sh

echo "==> [2/3] Installing any new Node.js dependencies..."
cd "${API_DIR}"
npm install --omit=dev

echo "==> [3/3] Restarting vltd-mobile-api via PM2..."
pm2 restart vltd-mobile-api || pm2 start ecosystem.config.js
pm2 save

sleep 2
echo "==> Health Check:"
curl -s http://127.0.0.1:3019/health || true
echo ""
echo "==> Mobile API updated! View logs: pm2 logs vltd-mobile-api"
