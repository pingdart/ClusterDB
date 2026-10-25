#!/usr/bin/env bash
# ==============================================================================
# PingDartDB Safe Linux Uninstaller
# Stops and deregisters service, removes binaries, and preserves data by default.
# ==============================================================================

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

BIN_DIR="/usr/local/bin"
CONFIG_DIR="/etc/pingdartdb"
DATA_DIR="/var/lib/pingdartdb"
LOG_DIR="/var/log/pingdartdb"
SERVICE_FILE="/etc/systemd/system/pingdartdb.service"
LOGROTATE_FILE="/etc/logrotate.d/pingdartdb"

PURGE=0
for arg in "$@"; do
    if [ "$arg" == "--purge" ]; then
        PURGE=1
    fi
done

echo -e "${RED}${BOLD}"
echo "============================================================"
echo "          PingDartDB Uninstallation Wizard                  "
echo "============================================================"
echo -e "${NC}"

if [ "${EUID}" -ne 0 ]; then
    echo -e "${RED}[ERROR] This uninstaller must be executed as root or with sudo.${NC}"
    exit 1
fi

# 1. Stop and Disable Service
echo -e "${BLUE}[1/4] Stopping and disabling systemd service...${NC}"
if systemctl is-active --quiet pingdartdb 2>/dev/null; then
    systemctl stop pingdartdb
    echo "Stopped service: pingdartdb"
fi

if systemctl is-enabled --quiet pingdartdb 2>/dev/null; then
    systemctl disable pingdartdb
    echo "Disabled service: pingdartdb"
fi

# 2. Remove Systemd & Logrotate Configurations
echo -e "${BLUE}[2/4] Removing service and logrotate registrations...${NC}"
if [ -f "${SERVICE_FILE}" ]; then
    rm -f "${SERVICE_FILE}"
    systemctl daemon-reload
    echo "Removed: ${SERVICE_FILE}"
fi

if [ -f "${LOGROTATE_FILE}" ]; then
    rm -f "${LOGROTATE_FILE}"
    echo "Removed: ${LOGROTATE_FILE}"
fi

# 3. Remove Binaries
echo -e "${BLUE}[3/4] Removing installed binaries...${NC}"
rm -f "${BIN_DIR}/pingdart-core" "${BIN_DIR}/pingdart" "${BIN_DIR}/pingdart-core.rollback" "${BIN_DIR}/pingdart.rollback"
echo "Removed: ${BIN_DIR}/pingdart-core"
echo "Removed: ${BIN_DIR}/pingdart"

# 4. Handle Data & Configuration Preservation
echo -e "${BLUE}[4/4] Evaluating data retention...${NC}"

if [ "${PURGE}" -eq 0 ] && [ -t 0 ]; then
    echo -e "\n${YELLOW}${BOLD}Safety Notice:${NC}"
    echo "By default, your database files and configuration are preserved."
    echo "  Config : ${CONFIG_DIR}"
    echo "  Data   : ${DATA_DIR}"
    echo "  Logs   : ${LOG_DIR}"
    echo ""
    read -rp "Do you want to PERMANENTLY delete all database data and configuration? Type 'DELETE' to confirm, or press Enter to keep: " CONFIRM_PURGE
    if [ "${CONFIRM_PURGE}" == "DELETE" ]; then
        PURGE=1
    fi
fi

if [ "${PURGE}" -eq 1 ]; then
    echo -e "${RED}[PURGE] Deleting data and configuration directories...${NC}"
    rm -rf "${CONFIG_DIR}"
    rm -rf "${DATA_DIR}"
    rm -rf "${LOG_DIR}"
    
    # Optionally remove system user
    if id -u pingdart >/dev/null 2>&1; then
        userdel pingdart 2>/dev/null || true
    fi
    if getent group pingdart >/dev/null 2>&1; then
        groupdel pingdart 2>/dev/null || true
    fi
    echo "Deleted all database data, configs, and service account."
else
    echo -e "${GREEN}[RETAINED] Database data and configurations were preserved.${NC}"
    echo "  Data stored at   : ${DATA_DIR}"
    echo "  Config stored at : ${CONFIG_DIR}"
    echo "  Logs stored at   : ${LOG_DIR}"
fi

echo -e "\n${GREEN}${BOLD}============================================================"
echo "          PingDartDB Uninstallation Complete               "
echo "============================================================${NC}"
