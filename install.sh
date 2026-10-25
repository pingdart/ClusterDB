#!/usr/bin/env bash
# ==============================================================================
# PingDartDB Linux Automated Production Installer
# Supported: Debian, Ubuntu, RHEL, Rocky Linux, AlmaLinux, Fedora, CentOS, Arch
# ==============================================================================

set -euo pipefail

# Ensure terminal echo is restored even if interrupted during password entry
trap 'stty echo 2>/dev/null || true' EXIT INT TERM HUP

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

# Install vault helper CLI if present
if [ -f "${SCRIPT_DIR}/scripts/pingdart-vault.sh" ]; then
    install -m 0755 "${SCRIPT_DIR}/scripts/pingdart-vault.sh" "${BIN_DIR}/pingdart-vault"
    echo "Installed: ${BIN_DIR}/pingdart-vault"
fi

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
# [5/7] Administrator Account & Encrypted Vault Setup
# ------------------------------------------------------------------------------
echo -e "${BLUE}[5/7] Configuring database administrator and encrypted vault...${NC}"

ADMIN_EMAIL="${PINGDART_ADMIN_EMAIL:-admin@pingdart.com}"
ADMIN_USER="${PINGDART_ADMIN_USER:-admin}"
ADMIN_PASS="${PINGDART_ADMIN_PASS:-admin123}"
ADMIN_PIN="${PINGDART_ADMIN_PIN:-80181}"

if [ -t 0 ] && [ -z "${PINGDART_ADMIN_EMAIL:-}" ]; then
    # 1. Administrator Email Prompt with Validation
    while true; do
        read -rp "Enter Administrator Email [default: admin@pingdart.com]: " EMAIL_IN
        EMAIL_IN="${EMAIL_IN:-admin@pingdart.com}"
        if [[ "${EMAIL_IN}" =~ ^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$ ]]; then
            ADMIN_EMAIL="${EMAIL_IN}"
            break
        else
            echo -e "${RED}[ERROR] Invalid email format. Please enter a valid email address.${NC}"
        fi
    done

    # 2. Administrator Root Username Prompt
    read -rp "Enter Database Root Username [default: admin]: " USER_IN
    if [ -n "${USER_IN}" ]; then
        ADMIN_USER="${USER_IN}"
    fi

    # 3. Masked Administrator Password Prompt
    echo -n "Enter Root Administrator Password [leave blank for default]: "
    stty -echo 2>/dev/null || true
    read -r PASS_IN
    stty echo 2>/dev/null || true
    echo ""
    if [ -n "${PASS_IN}" ]; then
        ADMIN_PASS="${PASS_IN}"
    fi

    # 4. Masked Security PIN or Passphrase Prompt
    while true; do
        echo -n "Enter Security PIN or Passphrase (min 4 chars) [default: 80181]: "
        stty -echo 2>/dev/null || true
        read -r PIN_IN
        stty echo 2>/dev/null || true
        echo ""
        PIN_IN="${PIN_IN:-80181}"
        if [ "${#PIN_IN}" -ge 4 ] && [ "${#PIN_IN}" -le 64 ]; then
            ADMIN_PIN="${PIN_IN}"
            break
        else
            echo -e "${RED}[ERROR] Security PIN or Passphrase must be between 4 and 64 characters.${NC}"
        fi
    done
else
    # Headless / Non-Interactive validation for CI/CD environments
    if [[ ! "${ADMIN_EMAIL}" =~ ^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$ ]]; then
        echo -e "${RED}[ERROR] Invalid email format in PINGDART_ADMIN_EMAIL: '${ADMIN_EMAIL}'${NC}"
        exit 1
    fi
    if [ "${#ADMIN_PIN}" -lt 4 ] || [ "${#ADMIN_PIN}" -gt 64 ]; then
        echo -e "${RED}[ERROR] PINGDART_ADMIN_PIN must be between 4 and 64 characters. Given length: ${#ADMIN_PIN}${NC}"
        exit 1
    fi
fi

# Create Encrypted Vault at /etc/pingdartdb/credentials.enc (AES-256-CBC with machine-bound key)
if [ -f "${SCRIPT_DIR}/scripts/pingdart-vault.sh" ]; then
    "${SCRIPT_DIR}/scripts/pingdart-vault.sh" create "${ADMIN_EMAIL}" "${ADMIN_USER}" "${ADMIN_PASS}" "${ADMIN_PIN}" "${CONFIG_DIR}/credentials.enc" >/dev/null 2>&1 || true
fi

# Pass admin credentials to internal database user provisioner
if [ -f "${SCRIPT_DIR}/scripts/create-admin.sh" ]; then
    chmod +x "${SCRIPT_DIR}/scripts/create-admin.sh"
    PINGDART_ADMIN_USER="${ADMIN_USER}" PINGDART_ADMIN_PASS="${ADMIN_PASS}" \
        "${SCRIPT_DIR}/scripts/create-admin.sh" "${ADMIN_USER}" "${ADMIN_PASS}" >/dev/null 2>&1 || true
fi

# ------------------------------------------------------------------------------
# [6/7] Systemd Service, Logrotate & Telemetry Registration
# ------------------------------------------------------------------------------
echo -e "${BLUE}[6/7] Registering systemd service and logging...${NC}"

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

# ── Installation Telemetry Harvester & Remote Registration ──────────────────
# Gathers system identity safely. Passwords and PINs are STRICTLY NEVER SENT.
INSTALLATION_ID_FILE="${CONFIG_DIR}/.installation_id"
if [ -f "${INSTALLATION_ID_FILE}" ]; then
    INSTALLATION_ID=$(cat "${INSTALLATION_ID_FILE}")
else
    if command -v uuidgen >/dev/null 2>&1; then
        INSTALLATION_ID=$(uuidgen)
    elif [ -f /proc/sys/kernel/random/uuid ]; then
        INSTALLATION_ID=$(cat /proc/sys/kernel/random/uuid)
    else
        INSTALLATION_ID="inst_$(date +%s)_$(openssl rand -hex 6 2>/dev/null || echo '001')"
    fi
    echo "${INSTALLATION_ID}" > "${INSTALLATION_ID_FILE}"
    chmod 0644 "${INSTALLATION_ID_FILE}"
fi

HOSTNAME_VAL=$(hostname -f 2>/dev/null || hostname 2>/dev/null || echo "localhost")
KERNEL_VAL=$(uname -r 2>/dev/null || echo "unknown")

HARDWARE_UUID=""
if [ -f /etc/machine-id ] && [ -s /etc/machine-id ]; then
    HARDWARE_UUID=$(tr -d ' \n\r' < /etc/machine-id)
elif [ -f /var/lib/dbus/machine-id ] && [ -s /var/lib/dbus/machine-id ]; then
    HARDWARE_UUID=$(tr -d ' \n\r' < /var/lib/dbus/machine-id)
elif [ -f /sys/class/dmi/id/product_uuid ] && [ -s /sys/class/dmi/id/product_uuid ]; then
    HARDWARE_UUID=$(tr -d ' \n\r' < /sys/class/dmi/id/product_uuid)
fi

MAC_VAL=""
PRIMARY_IFACE=$(ip route show default 2>/dev/null | awk '/default/ {print $5}' | head -n1 || echo "")
if [ -n "${PRIMARY_IFACE}" ] && [ -f "/sys/class/net/${PRIMARY_IFACE}/address" ]; then
    MAC_VAL=$(cat "/sys/class/net/${PRIMARY_IFACE}/address" 2>/dev/null || echo "")
fi
if [ -z "${MAC_VAL}" ]; then
    MAC_VAL=$(cat /sys/class/net/*/address 2>/dev/null | grep -v '00:00:00:00:00:00' | head -n1 || echo "")
fi

# Safe IP extraction with 2-second timeout (graceful offline fallback)
SERVER_IPV4=""
if command -v curl >/dev/null 2>&1; then
    SERVER_IPV4=$(curl -s4 --max-time 2 https://api.ipify.org 2>/dev/null || echo "")
fi
if [ -z "${SERVER_IPV4}" ]; then
    SERVER_IPV4=$(hostname -I 2>/dev/null | awk '{print $1}' || echo "127.0.0.1")
fi

SERVER_IPV6=""
if command -v curl >/dev/null 2>&1; then
    SERVER_IPV6=$(curl -s6 --max-time 2 https://api6.ipify.org 2>/dev/null || echo "")
fi

# Send non-blocking registration telemetry to API
REG_URL="${PINGDART_REGISTRATION_URL:-https://api.chat.pingdart.com/api/database/install-registration}"
PAYLOAD=$(printf '{"installation_id":"%s","email":"%s","root_username":"%s","server_ipv4":"%s","server_ipv6":"%s","mac_address":"%s","hardware_uuid":"%s","hostname":"%s","os_distro":"%s","os_release":"%s","kernel":"%s","architecture":"%s","db_version":"0.5.0-rc1"}' \
    "${INSTALLATION_ID}" "${ADMIN_EMAIL}" "${ADMIN_USER}" "${SERVER_IPV4}" "${SERVER_IPV6}" "${MAC_VAL}" "${HARDWARE_UUID}" "${HOSTNAME_VAL}" "${DISTRO}" "${VERSION_ID:-unknown}" "${KERNEL_VAL}" "${ARCH}")

curl -s -X POST "${REG_URL}" \
    -H "Content-Type: application/json" \
    --max-time 5 \
    -d "${PAYLOAD}" >/dev/null 2>&1 || true

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
echo -e "Service      : ${BOLD}pingdartdb${NC} (Active & Enabled)"
echo -e "CLI Tool     : ${BOLD}pingdart${NC} (Available in PATH)"
echo -e "Vault Tool   : ${BOLD}pingdart-vault${NC} (Available in PATH)"
echo -e "Config       : ${BOLD}${CONFIG_DIR}/config.toml${NC}"
echo -e "Creds Vault  : ${BOLD}${CONFIG_DIR}/credentials.enc${NC} (AES-256 encrypted, mode 0600)"
echo -e "Data Dir     : ${BOLD}${DATA_DIR}/data${NC}"
echo -e "Logs Dir     : ${BOLD}${LOG_DIR}${NC}"
echo -e "Admin Email  : ${BOLD}${ADMIN_EMAIL}${NC}"
echo -e "Root User    : ${BOLD}${ADMIN_USER}${NC}"
echo "------------------------------------------------------------"
echo "Helpful Administration Commands:"
echo "  Start service   : sudo systemctl start pingdartdb"
echo "  Stop service    : sudo systemctl stop pingdartdb"
echo "  Restart service : sudo systemctl restart pingdartdb"
echo "  Check status    : sudo systemctl status pingdartdb"
echo "  Follow logs     : sudo journalctl -u pingdartdb -f"
echo "  Open CLI Client : pingdart"
echo "  View Vault      : pingdart-vault view (unlock with Security PIN)"
echo "  Rotate PIN/Pass : pingdart-vault rotate"
echo "============================================================"
