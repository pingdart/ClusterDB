#!/usr/bin/env bash
# ==============================================================================
# PingDartDB Linux Automated Production Installer
# Supported: Debian, Ubuntu, RHEL, Rocky Linux, AlmaLinux, Fedora, CentOS, Arch
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
CONFIG_DIR="/etc/pingdartdb"
DATA_DIR="/var/lib/pingdartdb"
LOG_DIR="/var/log/pingdartdb"
SERVICE_FILE="/etc/systemd/system/pingdartdb.service"
LOGROTATE_FILE="/etc/logrotate.d/pingdartdb"

echo -e "${BLUE}${BOLD}"
echo "============================================================"
echo "          PingDartDB Enterprise Linux Installer             "
echo "============================================================"
echo -e "${NC}"

# Check for root / sudo privileges
if [ "${EUID}" -ne 0 ]; then
    echo -e "${RED}[ERROR] This installer must be executed as root or with sudo.${NC}"
    echo "Please re-run: sudo ./install.sh"
    exit 1
fi

# ------------------------------------------------------------------------------
# [1/7] Distribution & Architecture Detection
# ------------------------------------------------------------------------------
echo -e "${BLUE}[1/7] Detecting Linux distribution and architecture...${NC}"
ARCH=$(uname -m)
if [ "${ARCH}" != "x86_64" ] && [ "${ARCH}" != "aarch64" ]; then
    echo -e "${RED}[ERROR] Unsupported CPU architecture: ${ARCH}. Only x86_64 and aarch64 (ARM64) are supported.${NC}"
    exit 1
fi
echo "Architecture : ${ARCH}"

DISTRO="unknown"
if [ -f /etc/os-release ]; then
    . /etc/os-release
    DISTRO="${ID:-unknown}"
fi
echo "Distribution : ${DISTRO} (${VERSION_ID:-})"

# ------------------------------------------------------------------------------
# [2/7] System Dependencies
# ------------------------------------------------------------------------------
echo -e "${BLUE}[2/7] Checking and installing required dependencies...${NC}"

install_dependencies() {
    case "${DISTRO}" in
        ubuntu|debian|pop|linuxmint)
            export DEBIAN_FRONTEND=noninteractive
            apt-get update -qq
            apt-get install -y -qq build-essential pkg-config libssl-dev curl git
            ;;
        rhel|centos|rocky|almalinux|fedora)
            if command -v dnf >/dev/null 2>&1; then
                dnf install -y -q gcc gcc-c++ make pkgconfig openssl-devel curl git
            else
                yum install -y -q gcc gcc-c++ make pkgconfig openssl-devel curl git
            fi
            ;;
        arch|manjaro)
            pacman -Sy --noconfirm base-devel openssl curl git
            ;;
        *)
            echo -e "${YELLOW}[WARN] Unknown distribution '${DISTRO}'. Ensuring basic build tools exist...${NC}"
            ;;
    esac
}

install_dependencies

# Verify Rust Toolchain
if ! command -v cargo >/dev/null 2>&1; then
    echo -e "${YELLOW}[INFO] Rust/Cargo not found. Installing official Rust toolchain...${NC}"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    export PATH="${HOME}/.cargo/bin:${PATH}"
    # Also link cargo for root if installed in user directory
    if [ -f "/root/.cargo/bin/cargo" ]; then
        export PATH="/root/.cargo/bin:${PATH}"
    fi
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo -e "${RED}[ERROR] Failed to locate Cargo. Please ensure Rust is installed and in your PATH.${NC}"
    exit 1
fi
echo "Rust toolchain: $(cargo --version)"

# ------------------------------------------------------------------------------
# [3/7] Building Database Engine & CLI Binaries
# ------------------------------------------------------------------------------
echo -e "${BLUE}[3/7] Building high-performance release binaries...${NC}"
cd "${SCRIPT_DIR}"

# Build server and client binaries
cargo build --release --bin pingdart-core --bin pingdart

if [ ! -f "${SCRIPT_DIR}/target/release/pingdart-core" ] || [ ! -f "${SCRIPT_DIR}/target/release/pingdart" ]; then
    echo -e "${RED}[ERROR] Release build failed. Missing expected binaries in target/release/.${NC}"
    exit 1
fi

# Install binaries to /usr/local/bin
install -m 0755 "${SCRIPT_DIR}/target/release/pingdart-core" "${BIN_DIR}/pingdart-core"
install -m 0755 "${SCRIPT_DIR}/target/release/pingdart" "${BIN_DIR}/pingdart"

echo "Installed: ${BIN_DIR}/pingdart-core"
echo "Installed: ${BIN_DIR}/pingdart"

# ------------------------------------------------------------------------------
# [4/7] Dedicated Service Account & FHS Directory Creation
# ------------------------------------------------------------------------------
echo -e "${BLUE}[4/7] Creating service account and directories...${NC}"

# Create 'pingdart' group if not present
if ! getent group pingdart >/dev/null 2>&1; then
    groupadd -r pingdart
    echo "Created system group: pingdart"
fi

# Create 'pingdart' system user if not present
if ! id -u pingdart >/dev/null 2>&1; then
    useradd -r -g pingdart -d "${DATA_DIR}" -s /sbin/nologin -c "PingDartDB Service Account" pingdart
    echo "Created system user: pingdart"
fi

# Provision directories
mkdir -p "${CONFIG_DIR}"
mkdir -p "${DATA_DIR}/data"
mkdir -p "${DATA_DIR}/backups"
mkdir -p "${LOG_DIR}"

# Set permissions
chown -R root:pingdart "${CONFIG_DIR}"
chmod 0750 "${CONFIG_DIR}"

chown -R pingdart:pingdart "${DATA_DIR}"
chmod 0700 "${DATA_DIR}"
chmod 0700 "${DATA_DIR}/data"
chmod 0700 "${DATA_DIR}/backups"

chown -R pingdart:pingdart "${LOG_DIR}"
chmod 0750 "${LOG_DIR}"

# Safe configuration installation (Never silently overwrite existing configuration)
if [ -f "${CONFIG_DIR}/config.toml" ]; then
    echo -e "${YELLOW}[INFO] Existing configuration found at ${CONFIG_DIR}/config.toml. Preserving it.${NC}"
    echo "An updated template is saved at ${CONFIG_DIR}/config.toml.new"
    cp "${SCRIPT_DIR}/config/pingdartdb.example.toml" "${CONFIG_DIR}/config.toml.new"
    chown pingdart:pingdart "${CONFIG_DIR}/config.toml.new"
    chmod 0640 "${CONFIG_DIR}/config.toml.new"
else
    cp "${SCRIPT_DIR}/config/pingdartdb.example.toml" "${CONFIG_DIR}/config.toml"
    chown pingdart:pingdart "${CONFIG_DIR}/config.toml"
    chmod 0640 "${CONFIG_DIR}/config.toml"
    echo "Created configuration: ${CONFIG_DIR}/config.toml"
fi

# ------------------------------------------------------------------------------
# [5/7] Initial Administrator Account Setup
# ------------------------------------------------------------------------------
echo -e "${BLUE}[5/7] Configuring initial database administrator...${NC}"
ADMIN_USER="${PINGDART_ADMIN_USER:-ravi80181}"
ADMIN_PASS="${PINGDART_ADMIN_PASS:-RavindraChinnu12#}"

if [ -t 0 ] && [ -z "${PINGDART_ADMIN_USER:-}" ]; then
    read -rp "Enter Database Administrator Username [default: ravi80181]: " USER_IN
    if [ -n "${USER_IN}" ]; then
        ADMIN_USER="${USER_IN}"
    fi

    echo -n "Enter Administrator Password [leave blank for default]: "
    stty -echo 2>/dev/null || true
    read -r PASS_IN
    stty echo 2>/dev/null || true
    echo ""
    if [ -n "${PASS_IN}" ]; then
        ADMIN_PASS="${PASS_IN}"
    fi
fi

# Pass admin credentials directly to create-admin script without logging password
if [ -f "${SCRIPT_DIR}/scripts/create-admin.sh" ]; then
    chmod +x "${SCRIPT_DIR}/scripts/create-admin.sh"
    PINGDART_ADMIN_USER="${ADMIN_USER}" PINGDART_ADMIN_PASS="${ADMIN_PASS}" \
        "${SCRIPT_DIR}/scripts/create-admin.sh" "${ADMIN_USER}" "${ADMIN_PASS}" >/dev/null 2>&1 || true
fi

# ------------------------------------------------------------------------------
# [6/7] Systemd Service & Logrotate Registration
# ------------------------------------------------------------------------------
echo -e "${BLUE}[6/7] Registering systemd service and logrotate...${NC}"

if [ -f "${SCRIPT_DIR}/packaging/pingdartdb.service" ]; then
    cp "${SCRIPT_DIR}/packaging/pingdartdb.service" "${SERVICE_FILE}"
    chmod 0644 "${SERVICE_FILE}"
fi

if [ -f "${SCRIPT_DIR}/packaging/pingdartdb.logrotate" ]; then
    cp "${SCRIPT_DIR}/packaging/pingdartdb.logrotate" "${LOGROTATE_FILE}"
    chmod 0644 "${LOGROTATE_FILE}"
fi

systemctl daemon-reload
systemctl enable pingdartdb.service
systemctl restart pingdartdb.service

# ------------------------------------------------------------------------------
# [7/7] Post-Installation Health Verification
# ------------------------------------------------------------------------------
echo -e "${BLUE}[7/7] Verifying database service health...${NC}"

if [ -f "${SCRIPT_DIR}/scripts/health-check.sh" ]; then
    chmod +x "${SCRIPT_DIR}/scripts/health-check.sh"
    if ! "${SCRIPT_DIR}/scripts/health-check.sh" 15; then
        echo -e "${RED}[ERROR] Database health check failed after startup.${NC}"
        echo "Check logs: sudo journalctl -u pingdartdb -n 50 --no-pager"
        exit 1
    fi
fi

echo -e "\n${GREEN}${BOLD}============================================================"
echo "          PingDartDB Installation Completed!                "
echo "============================================================${NC}"
echo -e "Service  : ${BOLD}pingdartdb${NC} (Active & Enabled)"
echo -e "CLI Tool : ${BOLD}pingdart${NC} (Available in PATH)"
echo -e "Config   : ${BOLD}${CONFIG_DIR}/config.toml${NC}"
echo -e "Data Dir : ${BOLD}${DATA_DIR}/data${NC}"
echo -e "Logs Dir : ${BOLD}${LOG_DIR}${NC}"
echo -e "Admin    : ${BOLD}${ADMIN_USER}${NC}"
echo "------------------------------------------------------------"
echo "Helpful Administration Commands:"
echo "  Start service   : sudo systemctl start pingdartdb"
echo "  Stop service    : sudo systemctl stop pingdartdb"
echo "  Restart service : sudo systemctl restart pingdartdb"
echo "  Check status    : sudo systemctl status pingdartdb"
echo "  Follow logs     : sudo journalctl -u pingdartdb -f"
echo "  Open CLI Client : pingdart"
echo "============================================================"
