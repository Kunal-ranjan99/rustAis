#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# Install & Tune MySQL 8 on Ubuntu EC2 + Create Mobile API Database & User
# Usage: sudo bash ~/rustAis/scripts/setup_mysql.sh
# ==============================================================================

APP_USER="${SUDO_USER:-ubuntu}"
ENV_FILE="/home/${APP_USER}/rustAis/mobile-api/src/.env"

# Load DB credentials from mobile-api/src/.env if present
DB_NAME="vltd_mobile"
DB_USER="vltd_user"
DB_PASS="ChangeMeStrongPassword123!"

if [[ -f "${ENV_FILE}" ]]; then
  echo "==> Loading DB settings from ${ENV_FILE}..."
  while IFS='=' read -r k v; do
    [[ -z "$k" || "$k" == \#* ]] && continue
    v="${v%$'\r'}"
    case "$k" in
      DB_DATABASE) DB_NAME="$v" ;;
      DB_USER) DB_USER="$v" ;;
      DB_PASSWORD) DB_PASS="$v" ;;
    esac
  done < "${ENV_FILE}"
fi

echo "==> [1/4] Installing MySQL Server..."
apt-get update -y
DEBIAN_FRONTEND=noninteractive apt-get install -y mysql-server

echo "==> [2/4] Applying memory-safe MySQL configuration for EC2..."
TOTAL_MEM_MB=$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo)
if [[ "$TOTAL_MEM_MB" -lt 3072 ]]; then
  echo "    Detected ${TOTAL_MEM_MB}MB RAM -> using compact 128MB InnoDB buffer pool."
  cat << 'EOF' > /etc/mysql/mysql.conf.d/99-vltd-tuning.cnf
[mysqld]
innodb_buffer_pool_size = 128M
performance_schema = OFF
max_connections = 60
bind-address = 127.0.0.1
EOF
else
  echo "    Detected ${TOTAL_MEM_MB}MB RAM -> using 512MB InnoDB buffer pool."
  cat << 'EOF' > /etc/mysql/mysql.conf.d/99-vltd-tuning.cnf
[mysqld]
innodb_buffer_pool_size = 512M
max_connections = 150
bind-address = 127.0.0.1
EOF
fi

echo "==> [3/4] Enabling and starting MySQL..."
systemctl enable mysql
systemctl restart mysql

echo "==> [4/4] Creating database '${DB_NAME}' and user '${DB_USER}'..."
mysql -u root << SQL
CREATE DATABASE IF NOT EXISTS \`${DB_NAME}\` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
CREATE USER IF NOT EXISTS '${DB_USER}'@'localhost' IDENTIFIED BY '${DB_PASS}';
CREATE USER IF NOT EXISTS '${DB_USER}'@'127.0.0.1' IDENTIFIED BY '${DB_PASS}';
ALTER USER '${DB_USER}'@'localhost' IDENTIFIED BY '${DB_PASS}';
ALTER USER '${DB_USER}'@'127.0.0.1' IDENTIFIED BY '${DB_PASS}';
GRANT ALL PRIVILEGES ON \`${DB_NAME}\`.* TO '${DB_USER}'@'localhost';
GRANT ALL PRIVILEGES ON \`${DB_NAME}\`.* TO '${DB_USER}'@'127.0.0.1';
FLUSH PRIVILEGES;
SQL

echo ""
echo "======================================================================"
echo " MySQL Ready!"
echo "   Database : ${DB_NAME}"
echo "   User     : ${DB_USER}"
echo ""
echo " To import your SQL dump into '${DB_NAME}':"
echo "   mysql -u ${DB_USER} -p ${DB_NAME} < /path/to/your_backup.sql"
echo "======================================================================"
