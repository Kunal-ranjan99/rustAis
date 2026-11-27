# VLTD AIS-140 Rust TCP Ingestion Gateway (`rustAis`)

Ultra-lightweight, production-grade **Rust (Tokio + Axum)** TCP server for **AIS-140 / VLTD GPS trackers** with:

- **High-Concurrency TCP Server (Port `5001`)**: Semaphore-bounded (`5,000` concurrent trackers), `TCP_NODELAY`, `90s` idle timeout, `64 KB` per-socket memory cap, ASCII + Hex stream auto-decoding, and full AIS-140 `$PVT` validation (15-digit IMEI, `L`/`H` status, optional XOR checksum, GPS coordinates, speed, heading, ignition).
- **Triple-Layer Non-Blocking Storage**:
  - **Redis**: Instant latest live state (`packet:pvt:{imei}`, `devices:active`) via auto-reconnecting `ConnectionManager`.
  - **InfluxDB v2 (on AWS EBS)**: Time-series storage (`vltd_packet` measurement with `raw` + numeric `lat`, `lon`, `speed`, `heading`, `ignition`).
  - **Direct EBS Archive**: Daily-rotated append-only NDJSON backup (`/mnt/ebs/vltd_raw/ais140-YYYY-MM-DD.ndjson`).
- **Embedded WebGL Telemetry Dashboard (Port `8080`)**: Zero-Node, single-binary UI with **WebGL 3D Map** + **Native WebGL 3D Globe**, real-time **SSE live streaming**, **built-in multi-vehicle AIS-140 simulator** (injects real packets over TCP `:5001`), and a **Latest 20 Incoming Packets** live table.

---

## 1. AWS EC2 Security Group Inbound Rules

| Port | Protocol | Purpose |
| :--- | :--- | :--- |
| `5001` | TCP | AIS-140 / VLTD GPS tracker TCP ingestion |
| `8080` | TCP | WebGL Dashboard + HTTP REST API |
| `22` | TCP | SSH administration |
| `8086` | TCP | InfluxDB UI/API *(optional — restrict to your IP)* |

---

## 2. One-Command Fresh EC2 Setup & Permanent Startup

SSH into your Ubuntu EC2 instance and run:

```bash
curl -sSL https://raw.githubusercontent.com/Kunal-ranjan99/rustAis/main/scripts/setup.sh | bash
```

If you already attached an extra AWS EBS volume (e.g. `/dev/nvme1n1`), pass it in the same command to auto-mount and configure InfluxDB on EBS:

```bash
EBS_DEVICE=/dev/nvme1n1 curl -sSL https://raw.githubusercontent.com/Kunal-ranjan99/rustAis/main/scripts/setup.sh | bash
```

Once complete, open your WebGL Dashboard at:

```text
http://<YOUR_EC2_PUBLIC_IP>:8080
```

---

## 3. One-Command Future Update & Restart

Whenever you push new changes to GitHub, run this single command on your EC2 server:

```bash
bash ~/rustAis/scripts/update.sh
```

It pulls `main`, compiles the optimized release binary while the live server keeps running, restarts `vltd-gateway` cleanly, and verifies `/health`.

---

## 4. Using AWS EBS for InfluxDB v2 Storage

1. In the **AWS Console → EC2 → Volumes**, create an **EBS `gp3` volume** in the same Availability Zone as your EC2 instance and **Attach** it.
2. SSH into EC2 and check the device name:
   ```bash
   lsblk -o NAME,SIZE,TYPE,FSTYPE,MOUNTPOINT
   ```
   *(Typically `/dev/nvme1n1` on Nitro instances like `t3`/`t4g`/`m5`, or `/dev/xvdf` on older instances.)*
3. Run the automated EBS mount & InfluxDB migration script:
   ```bash
   sudo bash ~/rustAis/scripts/mount_ebs_influx.sh /dev/nvme1n1
   ```
   This script:
   - Formats the EBS volume as `ext4` **only if unformatted** (never wipes existing data).
   - Mounts it permanently at `/mnt/ebs` via UUID in `/etc/fstab`.
   - Migrates `/var/lib/influxdb` → `/mnt/ebs/influxdb` and configures `/etc/influxdb/config.toml`:
     ```toml
     bolt-path = "/mnt/ebs/influxdb/influxd.bolt"
     engine-path = "/mnt/ebs/influxdb/engine"
     ```
   - Sets up `/mnt/ebs/vltd_raw` for daily append-only raw AIS-140 NDJSON backups.

---

## 5. API & Service Reference

| Method | Path | Description |
| :--- | :--- | :--- |
| `GET` | `/` | Embedded WebGL 3D Telemetry Dashboard & Simulator |
| `GET` | `/health` | Gateway health, active TCP connections, packet & storage counters |
| `GET` | `/api/recent` | Latest **20** incoming AIS-140 packets (parsed + raw) |
| `GET` | `/api/live` | Latest parsed telemetry state for all active vehicles |
| `GET` | `/api/stream` | Real-time Server-Sent Events (SSE) telemetry stream |
| `POST` | `/api/simulate` | Sends a real AIS-140 `$PVT` packet over TCP to port `5001` |
| `GET` | `/devices` | Latest live raw packet for all tracked devices (Redis) |
| `GET` | `/devices/latest?imei=A,B` | Latest live raw packets for specific IMEIs (Redis) |
| `GET` | `/devices/:imei` | Single device live packet (Redis) |
| `GET` | `/influx/history?imei=X&start=0&end=0` | Historical raw packets from InfluxDB |

### Useful Commands on EC2

```bash
# Live server logs
sudo journalctl -u vltd-gateway -f

# Check service status
sudo systemctl status vltd-gateway redis-server influxdb

# Send a manual AIS-140 test packet via TCP
echo '$PVT,AIS140,1.0,NR,01,L,864251050012345,DL01AB1234,1,27112026,090000,28.613900,N,77.209000,E,52.3,90.0,11,215.0,0.9,0.8,AIRTEL,1,1,13.8,4.1,0*2F' | nc 127.0.0.1 5001
```
