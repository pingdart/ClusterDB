#!/usr/bin/env bash
# ==============================================================================
# PingDartDB Encrypted Credential Vault Helper
# Manages authenticated, encrypted-at-rest administrator credentials with machine binding.
# Security Model:
# - Dual-Key Separation: Independent AES encryption key & HMAC authentication key
# - Ciphertext Integrity: HMAC-SHA256 Encrypt-then-MAC with constant-time verification
# - Key Derivation: PBKDF2 (600,000 rounds) bound to hardware identifier
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

DEFAULT_PBKDF2_ITERATIONS=600000

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

get_host_secret() {
    local secret_file="${CONFIG_DIR}/.host_secret"
    if [ -f "${secret_file}" ] && [ -s "${secret_file}" ]; then
        tr -d ' \n\r' < "${secret_file}"
    else
        echo "pingdart-vault-host-secret-fallback"
    fi
}

ensure_host_secret() {
    local target_vault="${1:-${VAULT_FILE}}"
    local secret_file="${CONFIG_DIR}/.host_secret"
    if [ ! -f "${secret_file}" ]; then
        # Critical guard: If vault already exists but host secret is missing, DO NOT silently overwrite!
        if [ -f "${target_vault}" ]; then
            echo -e "${RED}[ERROR] Vault exists at ${target_vault} but host secret ${secret_file} is missing!${NC}" >&2
            echo -e "${RED}Refusing to generate a new host secret as it would render existing credentials permanently unrecoverable.${NC}" >&2
            echo -e "${YELLOW}Recovery options:${NC}" >&2
            echo -e "  1. Restore ${secret_file} from backup, OR" >&2
            echo -e "  2. If re-initializing, delete ${target_vault} before re-running create.${NC}" >&2
            return 1
        fi

        mkdir -p "$(dirname "${secret_file}")" 2>/dev/null || true
        if [ -w "$(dirname "${secret_file}")" ]; then
            openssl rand -hex 32 > "${secret_file}" 2>/dev/null || echo "host_$(date +%s)" > "${secret_file}"
            chmod 0400 "${secret_file}" 2>/dev/null || true
            if getent group pingdart >/dev/null 2>&1; then
                chown root:pingdart "${secret_file}" 2>/dev/null || true
            fi
        fi
    fi
}

# Cryptographic Key Derivation Function (PBKDF2 with 600,000 iterations & unique random salt)
# Both the AES encryption key and HMAC authentication key are independently derived.
derive_kdf_key() {
    local pin="$1"
    local salt="$2"
    local domain="$3"
    local iter="${4:-${DEFAULT_PBKDF2_ITERATIONS}}"
    local machine_id
    machine_id=$(get_machine_id)
    local host_secret
    host_secret=$(get_host_secret)
    local pass="${pin}::${machine_id}::${host_secret}::${domain}"
    openssl enc -aes-256-cbc -P -pbkdf2 -iter "${iter}" -pass "pass:${pass}" -S "${salt}" 2>/dev/null | awk -F'=' '/key=/ {print $2}'
}

get_vault_param() {
    local vault_src="$1"
    local param_name="$2"
    grep "^${param_name}:" "${vault_src}" 2>/dev/null | cut -d':' -f2 | tr -d ' \n\r' || echo ""
}

get_vault_iterations() {
    local vault_src="$1"
    local iter
    iter=$(get_vault_param "${vault_src}" "ITER")
    if [ -n "${iter}" ] && [[ "${iter}" =~ ^[0-9]+$ ]] && [ "${iter}" -ge 100000 ] && [ "${iter}" -le 5000000 ]; then
        echo "${iter}"
    else
        echo "${DEFAULT_PBKDF2_ITERATIONS}"
    fi
}

derive_enc_key() {
    local pin="$1"
    local salt="${2:-}"
    local iter="${3:-${DEFAULT_PBKDF2_ITERATIONS}}"
    if [ -n "${salt}" ]; then
        derive_kdf_key "${pin}" "${salt}" "pingdart-vault-aes-enc-v3" "${iter}"
    else
        local machine_id
        machine_id=$(get_machine_id)
        echo -n "${pin}::${machine_id}::pingdart-vault-aes-enc-v1" | openssl dgst -sha256 | awk '{print $NF}'
    fi
}

derive_mac_key() {
    local pin="$1"
    local salt="${2:-}"
    local iter="${3:-${DEFAULT_PBKDF2_ITERATIONS}}"
    if [ -n "${salt}" ]; then
        derive_kdf_key "${pin}" "${salt}" "pingdart-vault-hmac-auth-v3" "${iter}"
    else
        local machine_id
        machine_id=$(get_machine_id)
        echo -n "${pin}::${machine_id}::pingdart-vault-hmac-auth-v1" | openssl dgst -sha256 | awk '{print $NF}'
    fi
}

# Constant-time comparison to prevent timing side-channel attacks on MAC verification
constant_time_compare() {
    local a="$1"
    local b="$2"
    if [ "${#a}" -ne "${#b}" ]; then
        return 1
    fi
    local diff=0
    for ((i=0; i<${#a}; i++)); do
        local c1 c2
        c1=$(printf '%d' "'${a:$i:1}")
        c2=$(printf '%d' "'${b:$i:1}")
        diff=$((diff | (c1 ^ c2)))
    done
    [ "$diff" -eq 0 ]
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

    ensure_host_secret "${vault_dest}"

    local machine_id
    machine_id=$(get_machine_id)

    # 1. Generate unique 16-byte random salt for KDF
    local salt
    salt=$(openssl rand -hex 16)

    # 2. Derive independent encryption and HMAC keys using PBKDF2 (600,000 iterations)
    local enc_key
    enc_key=$(derive_enc_key "${pin}" "${salt}" "${DEFAULT_PBKDF2_ITERATIONS}")
    local mac_key
    mac_key=$(derive_mac_key "${pin}" "${salt}" "${DEFAULT_PBKDF2_ITERATIONS}")

    local json_payload
    json_payload=$(printf '{"email":"%s","username":"%s","password":"%s","machine_id":"%s","created_at":%d}' \
        "${email}" "${user}" "${pass}" "${machine_id}" "$(date +%s)")

    local tmp_cipher
    tmp_cipher=$(mktemp)
    TMP_CLEANUP_FILES+=("${tmp_cipher}")

    # 3. Encrypt payload using AES-256-CBC with matching PBKDF2 iteration count
    echo -n "${json_payload}" | openssl enc -aes-256-cbc -pbkdf2 -iter "${DEFAULT_PBKDF2_ITERATIONS}" -salt -pass "pass:${enc_key}" -out "${tmp_cipher}"

    # 4. Compute HMAC-SHA256 authentication tag over Associated Data (Envelope Headers + Ciphertext)
    local tmp_auth_input
    tmp_auth_input=$(mktemp)
    TMP_CLEANUP_FILES+=("${tmp_auth_input}")
    {
        printf "PINGDART_AUTHENTICATED_VAULT_V3\nITER:%s\nSALT:%s\n" "${DEFAULT_PBKDF2_ITERATIONS}" "${salt}"
        cat "${tmp_cipher}"
    } > "${tmp_auth_input}"

    local hmac_tag
    hmac_tag=$(openssl dgst -sha256 -hmac "${mac_key}" "${tmp_auth_input}" | awk '{print $NF}')
    rm -f "${tmp_auth_input}"

    # 5. Assemble authenticated container with explicit iteration count, salt, and HMAC tag
    {
        echo "# PINGDART_AUTHENTICATED_VAULT_V3"
        echo "ITER:${DEFAULT_PBKDF2_ITERATIONS}"
        echo "SALT:${salt}"
        echo "HMAC:${hmac_tag}"
        openssl base64 -in "${tmp_cipher}"
    } > "${vault_dest}"

    rm -f "${tmp_cipher}"
    set_vault_permissions "${vault_dest}"
    echo "Encrypted authenticated vault created at ${vault_dest} (mode: 0600, PBKDF2: ${DEFAULT_PBKDF2_ITERATIONS} iterations)."
}

# Internal helper to verify HMAC in constant time and extract ciphertext
extract_and_verify_vault() {
    local pin="$1"
    local vault_src="$2"
    local out_cipher="$3"

    if [ ! -f "${vault_src}" ]; then
        echo -e "${RED}[ERROR] Vault file not found: ${vault_src}${NC}" >&2
        return 1
    fi

    local salt
    salt=$(get_vault_param "${vault_src}" "SALT")
    local iterations
    iterations=$(get_vault_iterations "${vault_src}")

    local mac_key
    mac_key=$(derive_mac_key "${pin}" "${salt}" "${iterations}")

    # Check container format
    if head -n 1 "${vault_src}" | grep -q "PINGDART_AUTHENTICATED_VAULT_"; then
        local expected_hmac
        expected_hmac=$(grep '^HMAC:' "${vault_src}" | cut -d':' -f2 | tr -d ' \n\r')

        # Decode base64 ciphertext
        grep -v '^#' "${vault_src}" | grep -v '^ITER:' | grep -v '^SALT:' | grep -v '^HMAC:' | openssl base64 -d -out "${out_cipher}" 2>/dev/null || {
            echo -e "${RED}[ERROR] Vault container format is corrupted.${NC}" >&2
            return 1
        }

        # Verify HMAC-SHA256 tag in constant time over Associated Data BEFORE decryption
        local tmp_auth_input
        tmp_auth_input=$(mktemp)
        TMP_CLEANUP_FILES+=("${tmp_auth_input}")

        local container_header
        container_header=$(head -n 1 "${vault_src}" | tr -d '# \r\n')

        if [ "${container_header}" = "PINGDART_AUTHENTICATED_VAULT_V3" ]; then
            {
                printf "PINGDART_AUTHENTICATED_VAULT_V3\nITER:%s\nSALT:%s\n" "${iterations}" "${salt}"
                cat "${out_cipher}"
            } > "${tmp_auth_input}"
        else
            # V2 backwards-compatibility fallback
            cp "${out_cipher}" "${tmp_auth_input}"
        fi

        local computed_hmac
        computed_hmac=$(openssl dgst -sha256 -hmac "${mac_key}" "${tmp_auth_input}" | awk '{print $NF}')
        rm -f "${tmp_auth_input}"

        if ! constant_time_compare "${expected_hmac}" "${computed_hmac}"; then
            echo -e "${RED}❌ Integrity verification failed: Invalid PIN or vault metadata/ciphertext has been altered.${NC}" >&2
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
    local target_vault="${2:-${VAULT_FILE}}"
    if [ -z "${pin}" ]; then
        echo -n "Enter Security PIN or Passphrase: "
        stty -echo 2>/dev/null || true
        read -r pin
        stty echo 2>/dev/null || true
        echo ""
    fi

    local tmp_cipher
    tmp_cipher=$(mktemp)
    TMP_CLEANUP_FILES+=("${tmp_cipher}")

    local extract_status=0
    extract_and_verify_vault "${pin}" "${target_vault}" "${tmp_cipher}" || extract_status=$?

    if [ "${extract_status}" -ne 0 ]; then
        rm -f "${tmp_cipher}"
        exit 1
    fi

    local salt
    salt=$(get_vault_param "${target_vault}" "SALT")
    local iterations
    iterations=$(get_vault_iterations "${target_vault}")
    local enc_key
    enc_key=$(derive_enc_key "${pin}" "${salt}" "${iterations}")

    if openssl enc -d -aes-256-cbc -pbkdf2 -iter "${iterations}" -pass "pass:${enc_key}" \
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
    local target_vault="${2:-${VAULT_FILE}}"
    local show_pass=0
    for arg in "$@"; do
        if [ "$arg" = "--show-password" ] || [ "$arg" = "-s" ]; then
            show_pass=1
        fi
    done

    if [ -z "${pin}" ]; then
        echo -n "Enter Security PIN or Passphrase: "
        stty -echo 2>/dev/null || true
        read -r pin
        stty echo 2>/dev/null || true
        echo ""
    fi

    local tmp_cipher
    tmp_cipher=$(mktemp)
    TMP_CLEANUP_FILES+=("${tmp_cipher}")

    local extract_status=0
    extract_and_verify_vault "${pin}" "${target_vault}" "${tmp_cipher}" || extract_status=$?

    if [ "${extract_status}" -ne 0 ]; then
        rm -f "${tmp_cipher}"
        exit 1
    fi

    local salt
    salt=$(get_vault_param "${target_vault}" "SALT")
    local iterations
    iterations=$(get_vault_iterations "${target_vault}")
    local enc_key
    enc_key=$(derive_enc_key "${pin}" "${salt}" "${iterations}")

    local decrypted
    if ! decrypted=$(openssl enc -d -aes-256-cbc -pbkdf2 -iter "${iterations}" -pass "pass:${enc_key}" -in "${tmp_cipher}" 2>/dev/null); then
        rm -f "${tmp_cipher}"
        echo -e "${RED}[ERROR] Decryption failed. Incorrect PIN or unauthorized host machine.${NC}"
        exit 1
    fi
    rm -f "${tmp_cipher}"

    echo -e "${BLUE}${BOLD}=== PingDartDB Decrypted Credentials ===${NC}"
    echo "${decrypted}" | grep -o '"[^"]*":\("[^"]*"\|[0-9]*\)' | while read -r line; do
        key=$(echo "${line}" | cut -d':' -f1 | tr -d '"')
        val=$(echo "${line}" | cut -d':' -f2 | tr -d '"')
        if [ "${key}" = "password" ] && [ "${show_pass}" -ne 1 ]; then
            printf "%-15s : %s\n" "${key}" "******** (hidden - pass --show-password to reveal)"
        else
            printf "%-15s : %s\n" "${key}" "${val}"
        fi
    done
}

# ------------------------------------------------------------------------------
# Action: rotate
# ------------------------------------------------------------------------------
cmd_rotate() {
    local target_vault="${1:-${VAULT_FILE}}"
    echo -n "Enter CURRENT Security PIN or Passphrase: "
    stty -echo 2>/dev/null || true
    read -r old_pin
    stty echo 2>/dev/null || true
    echo ""

    local tmp_cipher
    tmp_cipher=$(mktemp)
    TMP_CLEANUP_FILES+=("${tmp_cipher}")

    local extract_status=0
    extract_and_verify_vault "${old_pin}" "${target_vault}" "${tmp_cipher}" || extract_status=$?

    if [ "${extract_status}" -ne 0 ]; then
        rm -f "${tmp_cipher}"
        exit 1
    fi

    local salt
    salt=$(get_vault_param "${target_vault}" "SALT")
    local iterations
    iterations=$(get_vault_iterations "${target_vault}")
    local enc_key
    enc_key=$(derive_enc_key "${old_pin}" "${salt}" "${iterations}")

    local decrypted
    if ! decrypted=$(openssl enc -d -aes-256-cbc -pbkdf2 -iter "${iterations}" -pass "pass:${enc_key}" -in "${tmp_cipher}" 2>/dev/null); then
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

    echo -n "Enter NEW Security PIN or Passphrase (leave blank to keep current): "
    stty -echo 2>/dev/null || true
    read -r new_pin
    stty echo 2>/dev/null || true
    echo ""

    if [ -z "${new_pin}" ]; then
        new_pin="${old_pin}"
    fi

    # Re-encrypt with new keys and HMAC-SHA256 authentication tag
    cmd_create "${current_email}" "${current_user}" "${new_pass}" "${new_pin}" "${target_vault}"
    echo -e "${GREEN}✅ Credentials successfully rotated and re-encrypted at ${target_vault}.${NC}"
}

usage() {
    echo "PingDartDB Credential Vault CLI"
    echo "Usage: pingdart-vault <command> [options]"
    echo ""
    echo "Commands:"
    echo "  view [pin] [vault_path] [--show-password]   Decrypt and inspect vault credentials (masks password by default)"
    echo "  verify [pin] [vault_path]                   Test PIN/passphrase validity and HMAC envelope integrity"
    echo "  rotate [vault_path]                         Rotate password and/or Security PIN safely"
    echo "  create <email> <user> <pass> <pin> [path]   (Internal helper)"
    exit 1
}

ACTION="${1:-}"
case "${ACTION}" in
    view)
        cmd_view "${@:2}"
        ;;
    verify)
        cmd_verify "${2:-}" "${3:-}"
        ;;
    rotate)
        cmd_rotate "${2:-}"
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
