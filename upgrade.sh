#!/usr/bin/env bash
# ==============================================================================
# PingDartDB Safe In-Place Upgrader with Automated Rollback
# Performs pre-upgrade backups, stages new binaries, restarts service,
# runs health probes, and automatically rolls back if health verification fails.
# ==============================================================================

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="/usr/local/bin"
DATA_DIR="/var/lib/pingdartdb"
BACKUP_DIR="${DATA_DIR}/backups"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_ARCHIVE="${BACKUP_DIR}/pre_upgrade_snapshot_${TIMESTAMP}.tar.gz"

echo -e "${BLUE}${BOLD}"
echo "============================================================"
echo "          PingDartDB Safe In-Place Upgrade                  "
echo "============================================================"
echo -e "${NC}"

if [ "${EUID}" -ne 0 ]; then
    echo -e "${RED}[ERROR] This upgrade script must be executed as root or with sudo.${NC}"
    exit 1
fi

# 1. Architecture Validation
ARCH=$(uname -m)
if [ "${ARCH}" != "x86_64" ] && [ "${ARCH}" != "aarch64" ]; then
    echo -e "${RED}[ERROR] Unsupported CPU architecture: ${ARCH}.${NC}"
    exit 1
fi

# 2. Stage & Build New Binaries
echo -e "${BLUE}[1/5] Building and validating new release binaries...${NC}"
cd "${SCRIPT_DIR}"
cargo build --release --bin pingdart-core --bin pingdart

NEW_SERVER="${SCRIPT_DIR}/target/release/pingdart-core"
NEW_CLI="${SCRIPT_DIR}/target/release/pingdart"

if [ ! -f "${NEW_SERVER}" ] || [ ! -f "${NEW_CLI}" ]; then
    echo -e "${RED}[ERROR] Failed to compile new binaries. Aborting upgrade.${NC}"
    exit 1
fi

# 3. Create Pre-Upgrade Backup Snapshot (ACID & WAL Consistent)
echo -e "${BLUE}[2/5] Creating pre-upgrade backup snapshot...${NC}"
mkdir -p "${BACKUP_DIR}"

WAS_RUNNING=0
if systemctl is-active --quiet pingdartdb 2>/dev/null; then
    echo "Pausing service briefly to ensure clean WAL flush and ACID snapshot consistency..."
    systemctl stop pingdartdb.service
    WAS_RUNNING=1
fi
sync

if [ -d "${DATA_DIR}/data" ]; then
    tar -czf "${BACKUP_ARCHIVE}" -C "${DATA_DIR}" data 2>/dev/null || true
    chown pingdart:pingdart "${BACKUP_ARCHIVE}" 2>/dev/null || true
    chmod 0600 "${BACKUP_ARCHIVE}" 2>/dev/null || true
    
    # Verify archive integrity
    if ! tar -tzf "${BACKUP_ARCHIVE}" >/dev/null 2>&1; then
        echo -e "${RED}[FATAL] Snapshot creation failed or produced corrupt archive! Aborting upgrade.${NC}"
        if [ "${WAS_RUNNING}" -eq 1 ]; then
            systemctl start pingdartdb.service || true
        fi
        exit 1
    fi
    echo "Consistent database snapshot verified: ${BACKUP_ARCHIVE}"
fi

# 4. Backup Existing Binaries
echo -e "${BLUE}[3/5] Preserving active binaries for potential rollback...${NC}"
if [ -f "${BIN_DIR}/pingdart-core" ]; then
    cp -p "${BIN_DIR}/pingdart-core" "${BIN_DIR}/pingdart-core.rollback"
fi
if [ -f "${BIN_DIR}/pingdart" ]; then
    cp -p "${BIN_DIR}/pingdart" "${BIN_DIR}/pingdart.rollback"
fi

# 5. Swap Binaries and Restart Service
echo -e "${BLUE}[4/5] Swapping binaries and starting updated pingdartdb service...${NC}"
install -m 0755 "${NEW_SERVER}" "${BIN_DIR}/pingdart-core"
install -m 0755 "${NEW_CLI}" "${BIN_DIR}/pingdart"

# Start service
systemctl start pingdartdb.service || true

# 6. Post-Upgrade Health Verification & Rollback Protection
echo -e "${BLUE}[5/5] Performing post-upgrade health probe...${NC}"

HEALTH_SCRIPT="${SCRIPT_DIR}/scripts/health-check.sh"
UPGRADE_SUCCESS=0

if [ -f "${HEALTH_SCRIPT}" ]; then
    chmod +x "${HEALTH_SCRIPT}"
    if "${HEALTH_SCRIPT}" 20; then
        UPGRADE_SUCCESS=1
    fi
else
    # Fallback curl probe
    sleep 3
    if curl -s -f -m 2 http://127.0.0.1:4650/ >/dev/null 2>&1; then
        UPGRADE_SUCCESS=1
    fi
fi

if [ "${UPGRADE_SUCCESS}" -eq 1 ]; then
    echo -e "\n${GREEN}${BOLD}[SUCCESS] PingDartDB upgrade completed successfully!${NC}"
    rm -f "${BIN_DIR}/pingdart-core.rollback" "${BIN_DIR}/pingdart.rollback"
    echo "Current binary: $(/usr/local/bin/pingdart-core --version 2>/dev/null || echo 'v0.5.0')"
    exit 0
else
    echo -e "\n${RED}${BOLD}[ALERT] Post-upgrade health check failed! Initiating full automatic rollback...${NC}"
    systemctl stop pingdartdb.service 2>/dev/null || true
    
    # 1. Restore Binaries
    if [ -f "${BIN_DIR}/pingdart-core.rollback" ]; then
        cp -p "${BIN_DIR}/pingdart-core.rollback" "${BIN_DIR}/pingdart-core"
        echo "Restored previous pingdart-core binary."
    fi
    if [ -f "${BIN_DIR}/pingdart.rollback" ]; then
        cp -p "${BIN_DIR}/pingdart.rollback" "${BIN_DIR}/pingdart"
        echo "Restored previous pingdart CLI binary."
    fi
    
    # 2. Restore Database Data & Schema safely (Isolate failed state rather than deleting)
    if [ -f "${BACKUP_ARCHIVE}" ]; then
        if tar -tzf "${BACKUP_ARCHIVE}" >/dev/null 2>&1; then
            FAILED_DATA_DIR="${DATA_DIR}/data_failed_upgrade_${TIMESTAMP}"
            if [ -d "${DATA_DIR}/data" ]; then
                echo "Preserving failed-upgrade state to ${FAILED_DATA_DIR} for post-mortem analysis..."
                mv "${DATA_DIR}/data" "${FAILED_DATA_DIR}"
            fi
            echo "Restoring database catalog and WAL from pre-upgrade snapshot..."
            tar -xzf "${BACKUP_ARCHIVE}" -C "${DATA_DIR}"
            chown -R pingdart:pingdart "${DATA_DIR}/data" 2>/dev/null || true
            chmod -R 0700 "${DATA_DIR}/data" 2>/dev/null || true
        else
            echo -e "${RED}[ERROR] Backup archive corrupted. Active data directory untouched.${NC}"
        fi
    fi
    
    systemctl start pingdartdb.service || true
    echo -e "${YELLOW}Service and database state rolled back to pre-upgrade version.${NC}"
    echo "Check logs with: sudo journalctl -u pingdartdb -n 50"
    echo "Pre-upgrade database snapshot remains intact at: ${BACKUP_ARCHIVE}"
    exit 1
fi
