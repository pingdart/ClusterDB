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

### 3. ACID-Consistent Upgrades & Zero-Downtime Hot Swap (`upgrade.sh` / `pingdart-upgrade`)
- **Remote Git Update Checking (`pingdart-upgrade check` / `pingdart check-update`)**: Inspects Git remote for new commits, releases, and changelogs on a live server without requiring root or modifying the running database.
- **Zero-Downtime Staging**: Compiles release binaries in the background while the database remains **100% online and actively serving client queries**.
- **Atomic Hot-Swap & Rapid Restart**:
  - Automatically executes filesystem `sync` and creates a verified pre-upgrade snapshot (`tar -tzf`).
  - Swaps active binaries atomically and triggers a sub-second service restart.
- **Automated Self-Healing Rollback**:
  - Executes comprehensive post-upgrade health probes (`health-check.sh`).
  - If verification fails, **automatically isolates the failed state** into `data_failed_upgrade_<TIMESTAMP>` (preventing data corruption) and rolls back both binaries and the verified database snapshot.

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

## 🚀 Installation & Upgrade Instructions

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

## 🔄 Live Server Upgrades & Remote Version Checking

### 1. Check for New Versions (While Database is Live)
Operators can check if new Git tags, commits, or releases exist without stopping the server or requiring root:
```bash
# Via upgrader CLI
pingdart-upgrade check

# Or via pingdart interactive client
pingdart check-update

# Or directly from the repository
./upgrade.sh check
```

### 2. Perform Zero-Downtime Live In-Place Upgrade & Auto-Restart
Compiles release binaries in the background while the database server continues serving active client traffic, then executes an atomic swap and sub-second restart:
```bash
# Single command live upgrade with automated backup & rollback
sudo pingdart-upgrade

# (or from the project repository)
sudo ./upgrade.sh
```
*Note: If health verification fails after upgrade, it automatically isolates the failed data and rolls back binary and database state to pre-upgrade snapshot.*

---

## 🔄 Disaster Recovery & Rollback Procedure

If manual rollback is ever required:
1. Stop the active service: `sudo systemctl stop pingdartdb`
2. Restore previous binary: `sudo cp /usr/local/bin/pingdart-core.rollback /usr/local/bin/pingdart-core`
3. Restore pre-upgrade snapshot: `sudo tar -xzf /var/lib/pingdartdb/backups/pre_upgrade_snapshot_<TIMESTAMP>.tar.gz -C /var/lib/pingdartdb`
4. Reset permissions: `sudo chown -R pingdart:pingdart /var/lib/pingdartdb/data && sudo chmod -R 0700 /var/lib/pingdartdb/data`
5. Restart service: `sudo systemctl start pingdartdb`
6. Run health probe: `/usr/local/bin/pingdart status`
