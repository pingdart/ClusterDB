#!/usr/bin/env bash
# ==============================================================================
# PingDartDB Release Packaging & SHA-256 Checksum Generator
# Bundles compiled binaries, installer scripts, and systemd units for release.
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${PINGDART_VERSION:-0.5.0}"
ARCH="${PINGDART_ARCH:-$(uname -m)}"
DIST_DIR="${SCRIPT_DIR}/dist"
BUNDLE_NAME="pingdartdb-v${VERSION}-linux-${ARCH}"
BUNDLE_DIR="${DIST_DIR}/${BUNDLE_NAME}"

echo "Building PingDartDB Release Bundle: ${BUNDLE_NAME}"
cd "${SCRIPT_DIR}"

# 1. Compile Release Binaries if not present
if [ ! -f "${SCRIPT_DIR}/target/release/pingdart-core" ] || [ ! -f "${SCRIPT_DIR}/target/release/pingdart" ]; then
    cargo build --release --bin pingdart-core --bin pingdart
fi

# 2. Stage Bundle Structure
rm -rf "${BUNDLE_DIR}"
mkdir -p "${BUNDLE_DIR}/bin"
mkdir -p "${BUNDLE_DIR}/config"
mkdir -p "${BUNDLE_DIR}/packaging"
mkdir -p "${BUNDLE_DIR}/scripts"

cp "${SCRIPT_DIR}/target/release/pingdart-core" "${BUNDLE_DIR}/bin/"
cp "${SCRIPT_DIR}/target/release/pingdart" "${BUNDLE_DIR}/bin/"
cp "${SCRIPT_DIR}/install.sh" "${BUNDLE_DIR}/"
cp "${SCRIPT_DIR}/upgrade.sh" "${BUNDLE_DIR}/"
cp "${SCRIPT_DIR}/uninstall.sh" "${BUNDLE_DIR}/"
cp "${SCRIPT_DIR}/config/pingdartdb.example.toml" "${BUNDLE_DIR}/config/"
cp "${SCRIPT_DIR}/packaging/pingdartdb.service" "${BUNDLE_DIR}/packaging/"
cp "${SCRIPT_DIR}/packaging/pingdartdb.logrotate" "${BUNDLE_DIR}/packaging/"
cp "${SCRIPT_DIR}/scripts/health-check.sh" "${BUNDLE_DIR}/scripts/"
cp "${SCRIPT_DIR}/scripts/create-admin.sh" "${BUNDLE_DIR}/scripts/"
cp "${SCRIPT_DIR}/scripts/pingdart-vault.sh" "${BUNDLE_DIR}/scripts/"
cp "${SCRIPT_DIR}/scripts/verify-backup-restore.sh" "${BUNDLE_DIR}/scripts/"
cp "${SCRIPT_DIR}/README.md" "${BUNDLE_DIR}/"

# 3. Create Tarball
cd "${DIST_DIR}"
tar -czf "${BUNDLE_NAME}.tar.gz" "${BUNDLE_NAME}"

# 4. Generate SHA-256 Checksums
if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${BUNDLE_NAME}.tar.gz" > "${BUNDLE_NAME}.sha256"
elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${BUNDLE_NAME}.tar.gz" > "${BUNDLE_NAME}.sha256"
fi

echo "============================================================"
echo "Release archive created: ${DIST_DIR}/${BUNDLE_NAME}.tar.gz"
echo "Checksum file created  : ${DIST_DIR}/${BUNDLE_NAME}.sha256"
cat "${DIST_DIR}/${BUNDLE_NAME}.sha256"
echo "============================================================"
