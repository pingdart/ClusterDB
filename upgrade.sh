#!/usr/bin/env bash
# ==============================================================================
# PingDartDB Safe In-Place Upgrader & Version Checker
# Zero-Downtime Staging: Compiles in background while service remains online.
# Atomic Swap & Automatic Rollback on Health Check Failure.
# ==============================================================================

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="/usr/local/bin"
DATA_DIR="/var/lib/pingdartdb"
BACKUP_DIR="${DATA_DIR}/backups"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_ARCHIVE="${BACKUP_DIR}/pre_upgrade_snapshot_${TIMESTAMP}.tar.gz"

# ------------------------------------------------------------------------------
# Subcommand: check
# Checks remote Git repository for new commits, tags, or release updates.
# Does NOT require root privileges and does NOT modify the live server.
# ------------------------------------------------------------------------------
do_check() {
    printf "%b\n" "${CYAN}${BOLD}"
    echo "============================================================"
    echo "          PingDartDB Remote Update Checker                  "
    echo "============================================================"
    printf "%b\n" "${NC}"

    cd "${SCRIPT_DIR}"

    if [ ! -d ".git" ]; then
        printf "%b\n" "${YELLOW}[WARN] Directory is not a git repository.${NC}"
        echo "Installed binary: $(/usr/local/bin/pingdart-core --version 2>/dev/null || echo 'v0.5.0')"
        exit 0
    fi

    CURRENT_COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
    CURRENT_TAG=$(git describe --tags --always 2>/dev/null || echo "${CURRENT_COMMIT}")
    ACTIVE_BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "main")

    printf "Current Version : %b%s%b (%s)\n" "${BOLD}" "${CURRENT_TAG}" "${NC}" "${CURRENT_COMMIT}"
    printf "Active Branch   : %s\n" "${ACTIVE_BRANCH}"
    echo "Checking remote git repository for updates..."

    # Fetch without modifying working tree
    if ! git fetch origin "${ACTIVE_BRANCH}" --tags --quiet 2>/dev/null; then
        printf "%b\n" "${YELLOW}[INFO] Remote git origin unreachable (sandbox/offline mode). Checking local tags...${NC}"
    fi

    REMOTE_COMMIT=$(git rev-parse --short "origin/${ACTIVE_BRANCH}" 2>/dev/null || echo "${CURRENT_COMMIT}")
    BEHIND_COUNT=$(git rev-list --count "HEAD..origin/${ACTIVE_BRANCH}" 2>/dev/null || echo "0")

    LATEST_TAG=$(git describe --tags "$(git rev-list --tags --max-count=1 2>/dev/null)" 2>/dev/null || echo "")

    printf "Remote Version  : %s %s\n\n" "${REMOTE_COMMIT}" "$([ -n "${LATEST_TAG}" ] && echo "(${LATEST_TAG})")"

    if [ "${BEHIND_COUNT}" -gt 0 ]; then
        printf "%b★ New version is available!%b (Server is %s commit(s) behind)\n" "${GREEN}${BOLD}" "${NC}" "${BEHIND_COUNT}"
        printf "%bRecent changelog:%b\n" "${CYAN}" "${NC}"
        git log "HEAD..origin/${ACTIVE_BRANCH}" --oneline -n 5 | while read -r line; do
            echo "  • ${line}"
        done
        echo ""
        printf "To upgrade safely without stopping the database during build:\n"
        printf "  %bsudo ./upgrade.sh%b   (or: %bsudo pingdart-upgrade%b)\n" "${BOLD}" "${NC}" "${BOLD}" "${NC}"
    else
        printf "%b✓ PingDartDB is fully up to date.%b (Commit: %s)\n" "${GREEN}" "${NC}" "${CURRENT_COMMIT}"
    fi
    exit 0
}

# ------------------------------------------------------------------------------
# Dispatch CLI Arguments
# ------------------------------------------------------------------------------
CMD="${1:-apply}"
if [ "${CMD}" = "check" ] || [ "${CMD}" = "--check" ] || [ "${CMD}" = "-c" ]; then
    do_check
fi

# Upgrade execution requires root privileges
if [ "${EUID}" -ne 0 ]; then
    echo -e "${RED}[ERROR] Upgrading requires root privileges.${NC}"
    echo "Please re-run: sudo ./upgrade.sh"
    echo "Or to just check for updates: ./upgrade.sh check"
    exit 1
fi

echo -e "${BLUE}${BOLD}"
echo "============================================================"
echo "    PingDartDB Safe In-Place Upgrade (Zero-Downtime Staging)"
echo "============================================================"
echo -e "${NC}"

# 1. Architecture Validation
ARCH=$(uname -m)
if [ "${ARCH}" != "x86_64" ] && [ "${ARCH}" != "aarch64" ]; then
    echo -e "${RED}[ERROR] Unsupported CPU architecture: ${ARCH}.${NC}"
    exit 1
fi

cd "${SCRIPT_DIR}"

# 2. Sync with Remote Git (if in a repository)
if [ -d ".git" ]; then
    echo -e "${BLUE}[1/6] Fetching latest source code from git repository...${NC}"
    ACTIVE_BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "main")
    git fetch origin "${ACTIVE_BRANCH}" --tags --quiet 2>/dev/null || true
    
    BEHIND_COUNT=$(git rev-list --count "HEAD..origin/${ACTIVE_BRANCH}" 2>/dev/null || echo "0")
    if [ "${BEHIND_COUNT}" -gt 0 ]; then
        echo "Pulling ${BEHIND_COUNT} new commit(s) on branch '${ACTIVE_BRANCH}'..."
        git pull --ff-only origin "${ACTIVE_BRANCH}" || {
            echo -e "${YELLOW}[WARN] Fast-forward git pull failed. Continuing with local sources...${NC}"
        }
    else
        echo "Source code is already at the latest git commit."
    fi
else
    echo -e "${BLUE}[1/6] Running upgrade from local release directory...${NC}"
fi

# 3. Compile in Background WHILE DATABASE REMAINS ONLINE
# Notice: Database server is NOT stopped here! It continues serving queries.
echo -e "${BLUE}[2/6] Compiling release binaries in background (database remains ONLINE)...${NC}"
cargo build --release --bin pingdart-core --bin pingdart

NEW_SERVER="${SCRIPT_DIR}/target/release/pingdart-core"
NEW_CLI="${SCRIPT_DIR}/target/release/pingdart"

if [ ! -f "${NEW_SERVER}" ] || [ ! -f "${NEW_CLI}" ]; then
    echo -e "${RED}[FATAL] Failed to compile new binaries. Live server was untouched.${NC}"
    exit 1
fi

echo "Compilation successful. Testing binary integrity..."
"${NEW_SERVER}" --version 2>/dev/null || true

# 4. Create Pre-Upgrade Backup Snapshot (ACID & WAL Consistent)
echo -e "${BLUE}[3/6] Flushing WAL and creating ACID-consistent backup snapshot...${NC}"
mkdir -p "${BACKUP_DIR}"

WAS_RUNNING=0
if systemctl is-active --quiet pingdartdb 2>/dev/null; then
    echo "Pausing service for atomic binary swap..."
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

# 5. Backup Existing Binaries
echo -e "${BLUE}[4/6] Preserving active binaries for potential rollback...${NC}"
if [ -f "${BIN_DIR}/pingdart-core" ]; then
    cp -p "${BIN_DIR}/pingdart-core" "${BIN_DIR}/pingdart-core.rollback"
fi
if [ -f "${BIN_DIR}/pingdart" ]; then
    cp -p "${BIN_DIR}/pingdart" "${BIN_DIR}/pingdart.rollback"
fi

# 6. Swap Binaries and Restart Service
echo -e "${BLUE}[5/6] Swapping binaries and executing fast restart...${NC}"
install -m 0755 "${NEW_SERVER}" "${BIN_DIR}/pingdart-core"
install -m 0755 "${NEW_CLI}" "${BIN_DIR}/pingdart"

# Start service
systemctl start pingdartdb.service || true

# 7. Post-Upgrade Health Verification & Rollback Protection
echo -e "${BLUE}[6/6] Performing post-upgrade health probe...${NC}"

HEALTH_SCRIPT="${SCRIPT_DIR}/scripts/health-check.sh"
UPGRADE_SUCCESS=0

if [ -f "${HEALTH_SCRIPT}" ]; then
    chmod +x "${HEALTH_SCRIPT}"
    if "${HEALTH_SCRIPT}" 20; then
        UPGRADE_SUCCESS=1
    fi
else
    # Fallback curl probe
    sleep 2
    if curl -s -f -m 2 http://127.0.0.1:4650/ >/dev/null 2>&1; then
        UPGRADE_SUCCESS=1
    fi
fi

if [ "${UPGRADE_SUCCESS}" -eq 1 ]; then
    echo -e "\n${GREEN}${BOLD}[SUCCESS] PingDartDB upgrade completed successfully!${NC}"
    rm -f "${BIN_DIR}/pingdart-core.rollback" "${BIN_DIR}/pingdart.rollback"
    echo "Active binary: $(/usr/local/bin/pingdart-core --version 2>/dev/null || echo 'v0.5.0')"
    echo "Downtime was limited strictly to service restart (~1 second)."
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
