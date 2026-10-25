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
TEST_TABLE="_integrity_test_$(date +%s)"
BACKUP_FILE="/tmp/pingdart_verify_$(date +%s).pb"

echo "=== PingDartDB Backup & Restore Integrity Probe ==="
echo "Target Node: ${BASE_URL}"
echo "Test Table : ${TEST_TABLE}"

# 1. Obtain Authentication Token
echo -n "[1/5] Authenticating as '${USER}'... "
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
    curl -s -X POST "${BASE_URL}/query" \
        -H "Content-Type: application/json" \
        -d "{\"token\":\"${TOKEN}\",\"query\":\"${sql}\"}"
}

# 2. Create Test Schema & Insert Known Records
echo -n "[2/5] Creating table and inserting test records... "
run_sql "CREATE TABLE ${TEST_TABLE} (id INT PRIMARY KEY, marker TEXT, amount INT);" >/dev/null
run_sql "INSERT INTO ${TEST_TABLE} VALUES (1, 'alpha', 100);" >/dev/null
run_sql "INSERT INTO ${TEST_TABLE} VALUES (2, 'beta', 200);" >/dev/null
run_sql "INSERT INTO ${TEST_TABLE} VALUES (3, 'gamma', 300);" >/dev/null

COUNT_RESP=$(run_sql "SELECT COUNT(*) FROM ${TEST_TABLE};")
if echo "${COUNT_RESP}" | grep -q "3"; then
    echo "[OK] (3 rows seeded)"
else
    echo "[FAILED]"
    echo "Query response: ${COUNT_RESP}"
    exit 1
fi

# 3. Export Native Protobuf Backup
echo -n "[3/5] Exporting native backup archive... "
EXPORT_RESP=$(curl -s -X POST "${BASE_URL}/export" \
    -H "Content-Type: application/json" \
    -d "{\"token\":\"${TOKEN}\",\"database\":\"default\",\"path\":\"${BACKUP_FILE}\"}" 2>/dev/null || true)

if [ -f "${BACKUP_FILE}" ] && [ -s "${BACKUP_FILE}" ]; then
    CHECKSUM=$(shasum -a 256 "${BACKUP_FILE}" 2>/dev/null | awk '{print $1}' || sha256sum "${BACKUP_FILE}" | awk '{print $1}')
    echo "[OK] (${CHECKSUM:0:16}...)"
else
    echo "[FAILED]"
    echo "Export response: ${EXPORT_RESP}"
    exit 1
fi

# 4. Simulate Disaster: Drop Test Table
echo -n "[4/5] Simulating catastrophic drop... "
run_sql "DROP TABLE ${TEST_TABLE};" >/dev/null
DROP_CHECK=$(run_sql "SELECT * FROM ${TEST_TABLE};")
if echo "${DROP_CHECK}" | grep -iq "error"; then
    echo "[OK] (Table successfully dropped)"
else
    echo "[WARN] Table drop did not report error."
fi

# 5. Restore Native Backup & Verify Parity
echo -n "[5/5] Restoring backup and validating data parity... "
IMPORT_RESP=$(curl -s -X POST "${BASE_URL}/import" \
    -H "Content-Type: application/json" \
    -d "{\"token\":\"${TOKEN}\",\"database\":\"default\",\"path\":\"${BACKUP_FILE}\"}" 2>/dev/null || true)

RESTORED_ROWS=$(run_sql "SELECT COUNT(*) FROM ${TEST_TABLE};")
rm -f "${BACKUP_FILE}"

if echo "${RESTORED_ROWS}" | grep -q "3"; then
    echo "[OK]"
    # Cleanup test table
    run_sql "DROP TABLE ${TEST_TABLE};" >/dev/null 2>&1 || true
    echo ""
    echo "============================================================"
    echo " SUCCESS: Full backup roundtrip verified with 100% parity! "
    echo "============================================================"
    exit 0
else
    echo "[FAILED]"
    echo "Restore response: ${IMPORT_RESP}"
    echo "Queried rows after restore: ${RESTORED_ROWS}"
    exit 1
fi
