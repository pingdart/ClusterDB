#!/usr/bin/env bash
# ==============================================================================
# PingDartDB Health-Check Probe
# Verifies operational readiness of the local or remote database node.
# ==============================================================================

set -euo pipefail

HOST="${PINGDART_HOST:-127.0.0.1}"
PORT="${PINGDART_PORT:-4650}"
TIMEOUT_SECONDS="${1:-15}"
ENDPOINT="http://${HOST}:${PORT}/"

echo -n "Checking PingDartDB health at ${ENDPOINT} "

START_TIME=$(date +%s)
while true; do
    if curl -s -f -m 2 -H "Accept: application/json" "${ENDPOINT}" >/dev/null 2>&1; then
        RESPONSE=$(curl -s -m 2 -H "Accept: application/json" "${ENDPOINT}" 2>/dev/null || true)
        STATUS=$(echo "${RESPONSE}" | grep -o '"status":"[^"]*"' | cut -d'"' -f4 || echo "unknown")
        VERSION=$(echo "${RESPONSE}" | grep -o '"version":"[^"]*"' | cut -d'"' -f4 || echo "unknown")
        
        if [ "${STATUS}" = "online" ]; then
            echo " [OK]"
            echo "Node status : ONLINE"
            echo "Version     : ${VERSION}"
            echo "Endpoint    : ${ENDPOINT}"
            exit 0
        fi
    fi

    CURRENT_TIME=$(date +%s)
    ELAPSED=$((CURRENT_TIME - START_TIME))
    if [ "${ELAPSED}" -ge "${TIMEOUT_SECONDS}" ]; then
        echo " [FAILED]"
        echo "Error: Database node at ${ENDPOINT} did not respond within ${TIMEOUT_SECONDS}s."
        exit 1
    fi

    echo -n "."
    sleep 1
done
