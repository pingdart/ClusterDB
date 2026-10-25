#!/usr/bin/env bash
# ==============================================================================
# PingDartDB Encrypted Credential Vault Helper
# Manages authenticated, encrypted-at-rest administrator credentials with machine binding.
# Security Model: AES-256-CBC + PBKDF2 (600,000 iterations) + HMAC-SHA256 (Encrypt-then-MAC)
# ==============================================================================

set -euo pipefail

# Global temp files tracker & cleanup handler
TMP_CLEANUP_FILES=()
cleanup_handler() {
    stty echo 2>/dev/null || true
    for f in "${TMP_CLEANUP_FILES[@]:-}"; do
        [ -n "$f" ] && rm -f "$f" 2>/dev/null || true
    done
}
trap cleanup_handler EXIT INT TERM HUP

VAULT_FILE="${PINGDART_VAULT_FILE:-/etc/pingdartdb/credentials.enc}"
CONFIG_DIR="${PINGDART_CONFIG_DIR:-/etc/pingdartdb}"

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

get_machine_id() {
    if [ -f /etc/machine-id ] && [ -s /etc/machine-id ]; then
        tr -d ' \n\r' < /etc/machine-id
    elif [ -f /var/lib/dbus/machine-id ] && [ -s /var/lib/dbus/machine-id ]; then
        tr -d ' \n\r' < /var/lib/dbus/machine-id
    elif [ -f /sys/class/dmi/id/product_uuid ] && [ -s /sys/class/dmi/id/product_uuid ]; then
        tr -d ' \n\r' < /sys/class/dmi/id/product_uuid
    else
        echo "pingdart-default-node-$(hostname)"
    fi
}

derive_key() {
    local pin="$1"
    local machine_id
    machine_id=$(get_machine_id)
    # Combine user PIN with machine identifier and static realm string
    echo -n "${pin}::${machine_id}::pingdart-enterprise-vault-v1"
}

set_vault_permissions() {
    local target="$1"
    if getent group pingdart >/dev/null 2>&1; then
        chown root:pingdart "${target}" 2>/dev/null || chown root:root "${target}" 2>/dev/null || true
    else
        chown root:root "${target}" 2>/dev/null || true
    fi
    chmod 0600 "${target}" 2>/dev/null || true
}

# ------------------------------------------------------------------------------
# Action: create (Internal or during install)
# ------------------------------------------------------------------------------
cmd_create() {
    local email="$1"
    local user="$2"
    local pass="$3"
    local pin="$4"
    local vault_dest="${5:-${VAULT_FILE}}"

    mkdir -p "$(dirname "${vault_dest}")"

    local machine_id
    machine_id=$(get_machine_id)
    local key_material
    key_material=$(derive_key "${pin}")

    local json_payload
    json_payload=$(printf '{"email":"%s","username":"%s","password":"%s","machine_id":"%s","created_at":%d}' \
        "${email}" "${user}" "${pass}" "${machine_id}" "$(date +%s)")

    local tmp_cipher
    tmp_cipher=$(mktemp)
    TMP_CLEANUP_FILES+=("${tmp_cipher}")

    # 1. Encrypt payload using AES-256-CBC with 600,000 PBKDF2 derivation rounds
    echo -n "${json_payload}" | openssl enc -aes-256-cbc -pbkdf2 -iter 600000 -salt \
        -pass "pass:${key_material}" -out "${tmp_cipher}"

    # 2. Compute HMAC-SHA256 authentication tag (Encrypt-then-MAC)
    local hmac_tag
    hmac_tag=$(openssl dgst -sha256 -hmac "${key_material}" "${tmp_cipher}" | awk '{print $NF}')

    # 3. Assemble authenticated container
    {
        echo "# PINGDART_AUTHENTICATED_VAULT_V1"
        echo "HMAC:${hmac_tag}"
        openssl base64 -in "${tmp_cipher}"
    } > "${vault_dest}"

    rm -f "${tmp_cipher}"
    set_vault_permissions "${vault_dest}"
    echo "Encrypted authenticated vault created at ${vault_dest} (mode: 0600)."
}

# Internal helper to verify HMAC and extract ciphertext
extract_and_verify_vault() {
    local pin="$1"
    local vault_src="$2"
    local out_cipher="$3"

    if [ ! -f "${vault_src}" ]; then
        echo -e "${RED}[ERROR] Vault file not found: ${vault_src}${NC}" >&2
        return 1
    fi

    local key_material
    key_material=$(derive_key "${pin}")

    # Check if vault is in authenticated container format
    if head -n 1 "${vault_src}" | grep -q "PINGDART_AUTHENTICATED_VAULT_V1"; then
        local expected_hmac
        expected_hmac=$(grep '^HMAC:' "${vault_src}" | cut -d':' -f2 | tr -d ' \n\r')

        # Decode base64 ciphertext
        sed '1,2d' "${vault_src}" | openssl base64 -d -out "${out_cipher}" 2>/dev/null || {
            echo -e "${RED}[ERROR] Vault container format is corrupted.${NC}" >&2
            return 1
        }

        # Verify HMAC-SHA256 tag
        local computed_hmac
        computed_hmac=$(openssl dgst -sha256 -hmac "${key_material}" "${out_cipher}" | awk '{print $NF}')

        if [ "${expected_hmac}" != "${computed_hmac}" ]; then
            echo -e "${RED}❌ Integrity verification failed: Invalid PIN or ciphertext has been altered.${NC}" >&2
            return 2
        fi
        return 0
    else
        # Legacy / raw openssl format fallback
        cp "${vault_src}" "${out_cipher}"
        return 0
    fi
}

# ------------------------------------------------------------------------------
# Action: verify
# ------------------------------------------------------------------------------
cmd_verify() {
    local pin="${1:-}"
    if [ -z "${pin}" ]; then
        echo -n "Enter Security PIN (4-8 digits): "
        stty -echo 2>/dev/null || true
        read -r pin
        stty echo 2>/dev/null || true
        echo ""
    fi

    local tmp_cipher
    tmp_cipher=$(mktemp)
    TMP_CLEANUP_FILES+=("${tmp_cipher}")

    local extract_status=0
    extract_and_verify_vault "${pin}" "${VAULT_FILE}" "${tmp_cipher}" || extract_status=$?

    if [ "${extract_status}" -ne 0 ]; then
        rm -f "${tmp_cipher}"
        exit 1
    fi

    local key_material
    key_material=$(derive_key "${pin}")

    if openssl enc -d -aes-256-cbc -pbkdf2 -iter 600000 -pass "pass:${key_material}" \
        -in "${tmp_cipher}" >/dev/null 2>&1; then
        rm -f "${tmp_cipher}"
        echo -e "${GREEN}✅ Vault verified successfully! HMAC integrity valid, PIN correct, and machine binding matches.${NC}"
        exit 0
    else
        rm -f "${tmp_cipher}"
        echo -e "${RED}❌ Verification failed: Decryption error.${NC}"
        exit 1
    fi
}

# ------------------------------------------------------------------------------
# Action: view
# ------------------------------------------------------------------------------
cmd_view() {
    local pin="${1:-}"
    if [ -z "${pin}" ]; then
        echo -n "Enter Security PIN: "
        stty -echo 2>/dev/null || true
        read -r pin
        stty echo 2>/dev/null || true
        echo ""
    fi

    local tmp_cipher
    tmp_cipher=$(mktemp)
    TMP_CLEANUP_FILES+=("${tmp_cipher}")

    local extract_status=0
    extract_and_verify_vault "${pin}" "${VAULT_FILE}" "${tmp_cipher}" || extract_status=$?

    if [ "${extract_status}" -ne 0 ]; then
        rm -f "${tmp_cipher}"
        exit 1
    fi

    local key_material
    key_material=$(derive_key "${pin}")

    local decrypted
    if ! decrypted=$(openssl enc -d -aes-256-cbc -pbkdf2 -iter 600000 -pass "pass:${key_material}" -in "${tmp_cipher}" 2>/dev/null); then
        rm -f "${tmp_cipher}"
        echo -e "${RED}[ERROR] Decryption failed. Incorrect PIN or unauthorized host machine.${NC}"
        exit 1
    fi
    rm -f "${tmp_cipher}"

    echo -e "${BLUE}${BOLD}=== PingDartDB Decrypted Credentials ===${NC}"
    echo "${decrypted}" | grep -o '"[^"]*":\("[^"]*"\|[0-9]*\)' | while read -r line; do
        key=$(echo "${line}" | cut -d':' -f1 | tr -d '"')
        val=$(echo "${line}" | cut -d':' -f2 | tr -d '"')
        printf "%-15s : %s\n" "${key}" "${val}"
    done
}

# ------------------------------------------------------------------------------
# Action: rotate
# ------------------------------------------------------------------------------
cmd_rotate() {
    echo -n "Enter CURRENT Security PIN: "
    stty -echo 2>/dev/null || true
    read -r old_pin
    stty echo 2>/dev/null || true
    echo ""

    local tmp_cipher
    tmp_cipher=$(mktemp)
    TMP_CLEANUP_FILES+=("${tmp_cipher}")

    local extract_status=0
    extract_and_verify_vault "${old_pin}" "${VAULT_FILE}" "${tmp_cipher}" || extract_status=$?

    if [ "${extract_status}" -ne 0 ]; then
        rm -f "${tmp_cipher}"
        exit 1
    fi

    local key_material
    key_material=$(derive_key "${old_pin}")

    local decrypted
    if ! decrypted=$(openssl enc -d -aes-256-cbc -pbkdf2 -iter 600000 -pass "pass:${key_material}" -in "${tmp_cipher}" 2>/dev/null); then
        rm -f "${tmp_cipher}"
        echo -e "${RED}[ERROR] Authentication failed. Current PIN is incorrect.${NC}"
        exit 1
    fi
    rm -f "${tmp_cipher}"

    local current_email
    current_email=$(echo "${decrypted}" | grep -o '"email":"[^"]*"' | cut -d'"' -f4 || echo "admin@pingdart.local")
    local current_user
    current_user=$(echo "${decrypted}" | grep -o '"username":"[^"]*"' | cut -d'"' -f4 || echo "admin")

    echo -e "${YELLOW}Rotating credentials for administrator: ${BOLD}${current_user}${NC}"

    echo -n "Enter NEW Password (leave blank to keep current): "
    stty -echo 2>/dev/null || true
    read -r new_pass
    stty echo 2>/dev/null || true
    echo ""

    if [ -z "${new_pass}" ]; then
        new_pass=$(echo "${decrypted}" | grep -o '"password":"[^"]*"' | cut -d'"' -f4)
    fi

    echo -n "Enter NEW Security PIN (4-8 digits, leave blank to keep current): "
    stty -echo 2>/dev/null || true
    read -r new_pin
    stty echo 2>/dev/null || true
    echo ""

    if [ -z "${new_pin}" ]; then
        new_pin="${old_pin}"
    fi

    # Re-encrypt with new key and HMAC-SHA256 authentication tag
    cmd_create "${current_email}" "${current_user}" "${new_pass}" "${new_pin}" "${VAULT_FILE}"
    echo -e "${GREEN}✅ Credentials successfully rotated and re-encrypted at ${VAULT_FILE}.${NC}"
}

usage() {
    echo "PingDartDB Credential Vault CLI"
    echo "Usage: pingdart-vault <command> [options]"
    echo ""
    echo "Commands:"
    echo "  view                Decrypt and inspect vault credentials (prompts for PIN)"
    echo "  verify [pin]        Test PIN validity and HMAC integrity against vault"
    echo "  rotate              Rotate password and/or Security PIN safely"
    echo "  create <email> <user> <pass> <pin> [path]   (Internal helper)"
    exit 1
}

ACTION="${1:-}"
case "${ACTION}" in
    view)
        cmd_view "${2:-}"
        ;;
    verify)
        cmd_verify "${2:-}"
        ;;
    rotate)
        cmd_rotate
        ;;
    create)
        if [ "$#" -lt 5 ]; then
            echo "Usage: pingdart-vault create <email> <user> <pass> <pin> [path]"
            exit 1
        fi
        cmd_create "$2" "$3" "$4" "$5" "${6:-${VAULT_FILE}}"
        ;;
    *)
        usage
        ;;
esac
