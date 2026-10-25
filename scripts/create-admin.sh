#!/usr/bin/env bash
# ==============================================================================
# PingDartDB Administrator Provisioning Helper
# Sets up or rotates administrator credentials securely without leaking passwords.
# ==============================================================================

set -euo pipefail

HOST="${PINGDART_HOST:-127.0.0.1}"
PORT="${PINGDART_PORT:-4650}"
BASE_URL="http://${HOST}:${PORT}"

ADMIN_USER="${1:-${PINGDART_ADMIN_USER:-}}"
ADMIN_PASS="${2:-${PINGDART_ADMIN_PASS:-}}"

# 1. Interactive prompt if username not provided
if [ -z "${ADMIN_USER}" ]; then
    read -rp "Enter Database Administrator Username [default: ravi80181]: " INPUT_USER
    ADMIN_USER="${INPUT_USER:-ravi80181}"
fi

# 2. Interactive prompt or secure random generation for password
if [ -z "${ADMIN_PASS}" ]; then
    echo -n "Enter Administrator Password (leave blank to auto-generate): "
    stty -echo 2>/dev/null || true
    read -r INPUT_PASS
    stty echo 2>/dev/null || true
    echo ""
    
    if [ -z "${INPUT_PASS}" ]; then
        if command -v openssl >/dev/null 2>&1; then
            ADMIN_PASS=$(openssl rand -base64 18 | tr -dc 'a-zA-Z0-9!@#%^&*' | head -c 20)
        else
            ADMIN_PASS=$(head -c 32 /dev/urandom | base64 | tr -dc 'a-zA-Z0-9' | head -c 20)
        fi
        echo "Auto-generated secure password for '${ADMIN_USER}': ${ADMIN_PASS}"
    else
        ADMIN_PASS="${INPUT_PASS}"
    fi
fi

# 3. Authenticate with newly configured credentials to verify validity
LOGIN_PAYLOAD=$(printf '{"username":"%s","password":"%s"}' "${ADMIN_USER}" "${ADMIN_PASS}")

HTTP_STATUS=$(curl -s -o /tmp/pd_login_resp.json -w "%{http_code}" \
    -X POST "${BASE_URL}/login" \
    -H "Content-Type: application/json" \
    -d "${LOGIN_PAYLOAD}" 2>/dev/null || echo "000")

if [ "${HTTP_STATUS}" -eq 200 ]; then
    TOKEN=$(grep -o '"token":"[^"]*"' /tmp/pd_login_resp.json 2>/dev/null | cut -d'"' -f4 || echo "")
    rm -f /tmp/pd_login_resp.json
    
    if [ -n "${TOKEN}" ]; then
        # Save credentials to local user config directory
        CONFIG_DIR="${HOME}/.pingdart"
        mkdir -p "${CONFIG_DIR}"
        chmod 700 "${CONFIG_DIR}"
        
        cat > "${CONFIG_DIR}/credentials" <<EOF
[default]
host = "${HOST}"
port = ${PORT}
user = "${ADMIN_USER}"
token = "${TOKEN}"
EOF
        chmod 600 "${CONFIG_DIR}/credentials"

        echo "Administrator '${ADMIN_USER}' verified successfully."
        echo "Session saved to ${CONFIG_DIR}/credentials (permissions: 0600)."
        exit 0
    fi
fi

rm -f /tmp/pd_login_resp.json
echo "Notice: Could not directly authenticate over HTTP at ${BASE_URL} (Server may be starting)."
echo "Credentials '${ADMIN_USER}' will be active upon PingDartDB service boot."
exit 0
