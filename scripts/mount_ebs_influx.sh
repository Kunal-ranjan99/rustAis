#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# Mount AWS EBS Volume at /mnt/ebs and Configure InfluxDB v2 + Raw AIS-140 Logs
# Usage: sudo bash scripts/mount_ebs_influx.sh /dev/nvme1n1
# ==============================================================================

DEVICE="${1:-${EBS_DEVICE:-}}"
MOUNT_POINT="/mnt/ebs"
APP_USER="${SUDO_USER:-ubuntu}"

if [[ -z "$DEVICE" ]]; then
  echo "Usage: sudo bash $0 <EBS_DEVICE>"
  echo "Example: sudo bash $0 /dev/nvme1n1   (or /dev/xvdf)"
  echo ""
  echo "Available block devices:"
  lsblk -o NAME,SIZE,TYPE,FSTYPE,MOUNTPOINT
  exit 1
fi

if [[ ! -b "$DEVICE" ]]; then
  echo "ERROR: Block device '$DEVICE' does not exist."
  lsblk -o NAME,SIZE,TYPE,FSTYPE,MOUNTPOINT
  exit 1
fi

echo "==> Checking filesystem on $DEVICE..."
if ! blkid "$DEVICE" >/dev/null 2>&1; then
  echo "==> Formatting $DEVICE as ext4 (first-time initialization)..."
  mkfs -t ext4 "$DEVICE"
else
  echo "==> Existing filesystem detected on $DEVICE (skipping format to preserve data)."
fi

mkdir -p "$MOUNT_POINT"

if ! mountpoint -q "$MOUNT_POINT"; then
  echo "==> Mounting $DEVICE -> $MOUNT_POINT..."
  mount "$DEVICE" "$MOUNT_POINT"
else
  echo "==> $MOUNT_POINT is already mounted."
fi

UUID=$(blkid -s UUID -o value "$DEVICE")
if ! grep -q "$UUID" /etc/fstab; then
  echo "==> Persisting mount in /etc/fstab (UUID=$UUID)..."
  echo "UUID=$UUID  $MOUNT_POINT  ext4  defaults,nofail  0  2" >> /etc/fstab
fi

# Create directories on EBS
mkdir -p "$MOUNT_POINT/influxdb/engine"
mkdir -p "$MOUNT_POINT/vltd_raw"
chown -R "$APP_USER:$APP_USER" "$MOUNT_POINT/vltd_raw"

# Configure InfluxDB v2 to store bolt + TSM/WAL engine on EBS
if systemctl list-unit-files | grep -q '^influxdb\.service'; then
  echo "==> Stopping InfluxDB to migrate storage paths to $MOUNT_POINT/influxdb..."
  systemctl stop influxdb || true

  if [[ -d /var/lib/influxdb && ! -f "$MOUNT_POINT/influxdb/influxd.bolt" ]]; then
    echo "==> Copying existing /var/lib/influxdb data to $MOUNT_POINT/influxdb..."
    cp -a /var/lib/influxdb/. "$MOUNT_POINT/influxdb/" || true
  fi

  chown -R influxdb:influxdb "$MOUNT_POINT/influxdb"
  chmod 750 "$MOUNT_POINT/influxdb"

  mkdir -p /etc/influxdb
  cat << 'EOF' > /etc/influxdb/config.toml
bolt-path = "/mnt/ebs/influxdb/influxd.bolt"
engine-path = "/mnt/ebs/influxdb/engine"
EOF
  chown influxdb:influxdb /etc/influxdb/config.toml

  echo "==> Starting InfluxDB on EBS storage..."
  systemctl enable influxdb
  systemctl start influxdb
  systemctl status influxdb --no-pager -l || true
fi

echo ""
echo "======================================================================"
echo "EBS volume $DEVICE is mounted at $MOUNT_POINT"
echo "  - InfluxDB v2 data : $MOUNT_POINT/influxdb"
echo "  - Raw AIS-140 logs : $MOUNT_POINT/vltd_raw"
echo "======================================================================"
df -h "$MOUNT_POINT"
