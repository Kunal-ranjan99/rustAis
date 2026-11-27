#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# Diagnose & Fix InfluxDB v2 Storage on EBS (/mnt/ebs/influxdb)
# Usage: sudo bash ~/rustAis/scripts/fix_influx.sh
# ==============================================================================

ORG="my-org"
BUCKET="vltd_data"
TOKEN="my-super-secret-influx-token"
EBS_INFLUX_DIR="/mnt/ebs/influxdb"

echo "==> [1/5] Fixing directory permissions for InfluxDB on /mnt/ebs..."
chmod 755 /mnt/ebs
mkdir -p "${EBS_INFLUX_DIR}/engine"
chown -R influxdb:influxdb "${EBS_INFLUX_DIR}"
chmod 750 "${EBS_INFLUX_DIR}"

echo "==> [2/5] Ensuring /etc/influxdb/config.toml points to EBS..."
mkdir -p /etc/influxdb
cat << 'EOF' > /etc/influxdb/config.toml
bolt-path = "/mnt/ebs/influxdb/influxd.bolt"
engine-path = "/mnt/ebs/influxdb/engine"
EOF
chown influxdb:influxdb /etc/influxdb/config.toml

echo "==> [3/5] Restarting InfluxDB service..."
systemctl daemon-reload
systemctl enable influxdb
systemctl restart influxdb

echo "==> Waiting for InfluxDB API (http://127.0.0.1:8086/health)..."
for i in $(seq 1 30); do
  if curl -sf http://127.0.0.1:8086/health >/dev/null 2>&1; then
    echo "    InfluxDB is UP!"
    break
  fi
  if [[ "$i" -eq 30 ]]; then
    echo "ERROR: InfluxDB failed to start. Showing recent logs:"
    journalctl -u influxdb -n 40 --no-pager
    exit 1
  fi
  sleep 1
done

echo "==> [4/5] Checking InfluxDB onboarding & test write..."
TEST_STATUS=$(curl -s -o /tmp/influx_test_resp.txt -w "%{http_code}" \
  -X POST "http://127.0.0.1:8086/api/v2/write?org=${ORG}&bucket=${BUCKET}&precision=ns" \
  -H "Authorization: Token ${TOKEN}" \
  -H "Content-Type: text/plain; charset=utf-8" \
  --data-binary "vltd_healthcheck,host=ec2 ok=1i")

if [[ "$TEST_STATUS" != "204" ]]; then
  echo "    Test write returned HTTP ${TEST_STATUS}: $(cat /tmp/influx_test_resp.txt)"
  echo "    Running influx setup for org=${ORG}, bucket=${BUCKET}..."
  influx setup \
    -n default \
    --host http://127.0.0.1:8086 \
    --username admin \
    --password "AdminPassword123!" \
    --org "${ORG}" \
    --bucket "${BUCKET}" \
    --token "${TOKEN}" \
    --force || true

  # Re-test write after setup
  TEST_STATUS=$(curl -s -o /tmp/influx_test_resp.txt -w "%{http_code}" \
    -X POST "http://127.0.0.1:8086/api/v2/write?org=${ORG}&bucket=${BUCKET}&precision=ns" \
    -H "Authorization: Token ${TOKEN}" \
    -H "Content-Type: text/plain; charset=utf-8" \
    --data-binary "vltd_healthcheck,host=ec2 ok=1i")
fi

echo "==> [5/5] Final InfluxDB Write Verification:"
if [[ "$TEST_STATUS" == "204" ]]; then
  echo "    SUCCESS! InfluxDB accepted write with HTTP 204 (storing on ${EBS_INFLUX_DIR})."
else
  echo "    FAILED (HTTP ${TEST_STATUS}): $(cat /tmp/influx_test_resp.txt)"
  echo "    Check /etc/influxdb/config.toml and journalctl -u influxdb -n 30"
  exit 1
fi

rm -f /tmp/influx_test_resp.txt
echo "==> Restarting vltd-gateway to reset error counters..."
systemctl restart vltd-gateway || true
sleep 2
curl -s http://127.0.0.1:8080/health || true
echo ""
