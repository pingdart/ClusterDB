# PingDartDB v0.5.0 Release Candidate 1 (v0.5.0-rc1)

**Release Date:** October 25, 2026  
**Commit:** `c82e6c9`  
**License:** Apache 2.0 / Proprietary  

PingDartDB `v0.5.0-rc1` introduces automated Linux production packaging, standard FHS directory management, a MySQL-style interactive CLI (`pingdart`), and ACID-safe upgrade/rollback automation.

---

## 🌟 What's New in v0.5.0

### 1. Enterprise Linux Installer (`install.sh`)
- Automated detection for **Ubuntu, Debian, RHEL, Rocky Linux, AlmaLinux, Fedora, and Arch**.
- Auto-provisions isolated `pingdart:pingdart` service account with sandboxed permissions (`0700`).
- Registers production-hardened `pingdartdb.service` and `pingdartdb.logrotate`.
- Never overwrites existing configurations silently.

### 2. MySQL-Style Interactive CLI (`pingdart`)
- Full interactive REPL with session caching in `~/.pingdart/session` (`0600`).
- ASCII table rendering for SQL queries (`SELECT`, `SHOW DATABASES`, `SHOW TABLES`).
- Non-interactive script execution via `pingdart -e "<SQL>"`.
- Secure credential management via `PINGDART_PASSWORD` environment variable to prevent `/proc` process snooping.

### 3. ACID-Consistent Upgrades & Automatic Rollback (`upgrade.sh`)
- Pauses the database service and executes filesystem `sync` before snapshotting, guaranteeing clean WAL state.
- Automatically verifies snapshot readability (`tar -tzf`) prior to replacing binaries.
- If post-upgrade health checks fail, **automatically isolates the failed state** into `data_failed_upgrade_<TIMESTAMP>` (preventing data loss) and restores the verified snapshot and previous binary.

### 4. Storage & Query Optimization Engine
- **SlottedPage & BufferPoolManager**: Page-level memory caching with Clock replacement.
- **MVCC & Vacuum**: Multi-version concurrency control with non-blocking snapshot reads.
- **Cost-Based Optimizer (CBO)**: Selects hash joins, index scans, or materialized view rewrites based on table statistics.
- **Parallel Query Execution**: Multi-threaded chunk scanning and distributed scatter-gather aggregations.

---

## 📦 Release Artifacts & Verification

| Asset | Target Architecture | SHA-256 Checksum |
| :--- | :--- | :--- |
| `pingdartdb-v0.5.0-linux-x86_64.tar.gz` | Linux x86_64 (AMD64) | *(Generated on build)* |
| `pingdartdb-v0.5.0-linux-arm64.tar.gz` | Linux aarch64 (ARM64) | `918a841629fee9a49be25f93cdb6041e290442a1b7fda40bdb375bbfabf065be` |

### Checksum Verification
```bash
sha256sum -c pingdartdb-v0.5.0-linux-arm64.sha256
# Expected output: pingdartdb-v0.5.0-linux-arm64.tar.gz: OK
```

---

## 🚀 Installation Instructions

### Option A: Install from Release Tarball (Recommended for Staging/Production)
```bash
curl -fsSLO https://github.com/pingdart/ClusterDB/releases/download/v0.5.0-rc1/pingdartdb-v0.5.0-linux-arm64.tar.gz
tar -xzf pingdartdb-v0.5.0-linux-arm64.tar.gz
cd pingdartdb-v0.5.0-linux-arm64
sudo ./install.sh
```

### Option B: Build & Install from Source
```bash
git clone https://github.com/pingdart/ClusterDB.git
cd ClusterDB
sudo ./install.sh
```

---

## 🔄 Disaster Recovery & Rollback Procedure

If manual rollback is ever required:
1. Stop the active service: `sudo systemctl stop pingdartdb`
2. Restore previous binary: `sudo cp /usr/local/bin/pingdart-core.rollback /usr/local/bin/pingdart-core`
3. Restore pre-upgrade snapshot: `sudo tar -xzf /var/lib/pingdartdb/backups/pre_upgrade_snapshot_<TIMESTAMP>.tar.gz -C /var/lib/pingdartdb`
4. Reset permissions: `sudo chown -R pingdart:pingdart /var/lib/pingdartdb/data && sudo chmod -R 0700 /var/lib/pingdartdb/data`
5. Restart service: `sudo systemctl start pingdartdb`
6. Run health probe: `/usr/local/bin/pingdart status`
