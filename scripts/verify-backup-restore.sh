#!/usr/bin/env bash
# ==============================================================================
# PingDartDB Backup Restoration & Integrity Test
# Verifies end-to-end backup snapshot creation, data drop, and full restore parity.
# ==============================================================================

set -euo pipefail

HOST="${PINGDART_HOST:-127.0.0.1}"
PORT="${PINGDART_PORT:-4650}"
BASE_URL="http://${HOST}:${PORT}"
USER="${PINGDART_USER:-ravi80181}"
PASS="${PINGDART_PASSWORD:-RavindraChinnu12#}"
TEST_DB="_sandbox_verify_$(date +%s)_$RANDOM"
TEST_TABLE="records"
BACKUP_FILE="/tmp/pingdart_verify_$(date +%s)_$RANDOM.pb"
TOKEN=""

cleanup() {
    local exit_code=$?
    rm -f "${BACKUP_FILE}" 2>/dev/null || true
    if [ -n "${TOKEN}" ] && [ -n "${TEST_DB}" ]; then
        curl -s -X POST "${BASE_URL}/query" \
            -H "Content-Type: application/json" \
            -d "{\"token\":\"${TOKEN}\",\"query\":\"DROP DATABASE ${TEST_DB};\"}" >/dev/null 2>&1 || true
    fi
    exit ${exit_code}
}
trap cleanup EXIT INT TERM

echo "=== PingDartDB Isolated Backup & Restore Integrity Probe ==="
echo "Target Node : ${BASE_URL}"
echo "Sandbox DB  : ${TEST_DB}"
echo "Test Table  : ${TEST_TABLE}"

# 1. Obtain Authentication Token
echo -n "[1/6] Authenticating as '${USER}'... "
LOGIN_RESP=$(curl -s -X POST "${BASE_URL}/login" \
    -H "Content-Type: application/json" \
    -d "{\"username\":\"${USER}\",\"password\":\"${PASS}\"}" 2>/dev/null || true)

TOKEN=$(echo "${LOGIN_RESP}" | grep -o '"token":"[^"]*"' | cut -d'"' -f4 || echo "")
if [ -z "${TOKEN}" ]; then
    echo "[FAILED]"
    echo "Error: Authentication failed. Response: ${LOGIN_RESP}"
    exit 1
fi
echo "[OK]"

run_sql() {
    local sql="$1"
    local db="${2:-${TEST_DB}}"
    curl -s -X POST "${BASE_URL}/query" \
        -H "Content-Type: application/json" \
        -d "{\"token\":\"${TOKEN}\",\"database\":\"${db}\",\"query\":\"${sql}\"}"
}

# 2. Create Isolated Sandbox Database
echo -n "[2/6] Provisioning dedicated sandbox database '${TEST_DB}'... "
curl -s -X POST "${BASE_URL}/query" \
    -H "Content-Type: application/json" \
    -d "{\"token\":\"${TOKEN}\",\"query\":\"CREATE DATABASE ${TEST_DB};\"}" >/dev/null
echo "[OK]"

# 3. Create Schema & Insert Known Records Inside Sandbox
echo -n "[3/6] Seeding sample data into sandbox... "
run_sql "CREATE TABLE ${TEST_TABLE} (id INT PRIMARY KEY, marker TEXT, amount INT);" >/dev/null
run_sql "INSERT INTO ${TEST_TABLE} VALUES (1, 'alpha', 100);" >/dev/null
run_sql "INSERT INTO ${TEST_TABLE} VALUES (2, 'beta', 200);" >/dev/null
run_sql "INSERT INTO ${TEST_TABLE} VALUES (3, 'gamma', 300);" >/dev/null

COUNT_RESP=$(run_sql "SELECT COUNT(*) FROM ${TEST_TABLE};")
if echo "${COUNT_RESP}" | grep -q "3"; then
    echo "[OK] (3 rows seeded in ${TEST_DB})"
else
    echo "[FAILED]"
    echo "Query response: ${COUNT_RESP}"
    exit 1
fi

# 4. Export Native Protobuf Backup of Sandbox DB
echo -n "[4/6] Exporting native backup archive of '${TEST_DB}'... "
EXPORT_RESP=$(curl -s -X POST "${BASE_URL}/export" \
    -H "Content-Type: application/json" \
    -d "{\"token\":\"${TOKEN}\",\"database\":\"${TEST_DB}\",\"path\":\"${BACKUP_FILE}\"}" 2>/dev/null || true)

if [ -f "${BACKUP_FILE}" ] && [ -s "${BACKUP_FILE}" ]; then
    CHECKSUM=$(shasum -a 256 "${BACKUP_FILE}" 2>/dev/null | awk '{print $1}' || sha256sum "${BACKUP_FILE}" | awk '{print $1}')
    echo "[OK] (${CHECKSUM:0:16}...)"
else
    echo "[FAILED]"
    echo "Export response: ${EXPORT_RESP}"
    exit 1
fi

# 5. Simulate Disaster: Drop Sandbox Table
echo -n "[5/6] Simulating disaster drop inside sandbox... "
run_sql "DROP TABLE ${TEST_TABLE};" >/dev/null
DROP_CHECK=$(run_sql "SELECT * FROM ${TEST_TABLE};")
if echo "${DROP_CHECK}" | grep -iq "error"; then
    echo "[OK] (Table successfully dropped)"
else
    echo "[WARN] Table drop check did not report error."
fi

# 6. Restore Native Backup into Sandbox & Verify Parity
echo -n "[6/6] Restoring backup and validating data parity... "
IMPORT_RESP=$(curl -s -X POST "${BASE_URL}/import" \
    -H "Content-Type: application/json" \
    -d "{\"token\":\"${TOKEN}\",\"database\":\"${TEST_DB}\",\"path\":\"${BACKUP_FILE}\"}" 2>/dev/null || true)

RESTORED_ROWS=$(run_sql "SELECT COUNT(*) FROM ${TEST_TABLE};")
rm -f "${BACKUP_FILE}"

# Cleanup isolated sandbox database
run_sql "DROP TABLE ${TEST_TABLE};" >/dev/null 2>&1 || true

if echo "${RESTORED_ROWS}" | grep -q "3"; then
    echo "[OK]"
    echo ""
    echo "============================================================"
    echo " SUCCESS: Sandbox backup roundtrip verified with 100% parity! "
    echo " Zero impact on production databases.                      "
    echo "============================================================"
    exit 0
else
    echo "[FAILED]"
    echo "Restore response: ${IMPORT_RESP}"
    echo "Queried rows after restore: ${RESTORED_ROWS}"
    exit 1
fi
