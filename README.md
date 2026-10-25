# PingDartDB

**PingDartDB** is an enterprise AI-native, multi-model distributed database engine built for high-throughput OLTP, columnar OLAP analytics, HNSW vector search, Raft replication, and MySQL-style SQL administration.

---

## 🚀 Quick Installation

### Standard Git Install (Linux)

```bash
git clone https://github.com/pingdart/ClusterDB.git pingdartdb
cd pingdartdb
chmod +x install.sh
sudo ./install.sh
```

The automated installer performs:
1. Linux distribution and CPU architecture detection.
2. System dependency and Rust toolchain verification.
3. High-performance release compilation (`pingdart-core`, `pingdart`, and `pingdart-vault`).
4. Creation of dedicated `pingdart:pingdart` service account.
5. FHS directory provisioning: `/etc/pingdartdb`, `/var/lib/pingdartdb/data`, `/var/log/pingdartdb`.
6. Interactive setup prompts for **Administrator Email**, **Root Username**, **Password**, and **4-8 digit Security PIN**.
7. Creates machine-bound AES-256 encrypted vault (`/etc/pingdartdb/credentials.enc`, mode `0600`).
8. Registers node telemetry via Node.js registration API (`/api/database/install-registration`). Passwords and PINs are **strictly never transmitted**.
9. Systemd service registration and post-install health verification.

### Non-Interactive / Headless Installation
For CI/CD and automated setups, pass environment variables:
```bash
sudo PINGDART_ADMIN_EMAIL="admin@example.com" \
     PINGDART_ADMIN_USER="admin" \
     PINGDART_ADMIN_PASS="StrongSecret123!" \
     PINGDART_ADMIN_PIN="80181" \
     ./install.sh
```

---

## 🔐 Encrypted Credential Vault (`pingdart-vault`)

PingDartDB stores root credentials in an encrypted-at-rest vault bound to the server hardware (`/etc/pingdartdb/credentials.enc`). It uses AES-256-CBC with PBKDF2 (600,000 derivation rounds) keyed by combining the administrator's **Security PIN** with the server's unique `/etc/machine-id`. Even if the file is copied off-machine, it cannot be decrypted.

Manage credentials securely via the native helper:

```bash
# View decrypted credentials in-memory (prompts for Security PIN)
pingdart-vault view

# Verify Security PIN validity against the encrypted vault
pingdart-vault verify

# Rotate administrator password or Security PIN
pingdart-vault rotate
```

---

## 🛠️ Command-Line Interface (`pingdart`)

PingDartDB includes a native MySQL-style CLI client:

```bash
# Interactive REPL with cached session
pingdart

# Show database status
pingdart status

# Connect to custom host/port
pingdart -h 127.0.0.1 -P 4650

# Execute single SQL query
pingdart -e "SHOW DATABASES;"
```

### Interactive REPL Session Example:
```text
PingDartDB CLI v0.5.0
Connected to http://127.0.0.1:4650

Username [ravi80181]: ravi80181
Password: ********
Authentication successful for user 'ravi80181'.

pingdart> SHOW DATABASES;
pingdart> USE default;
pingdart> CREATE TABLE users (id INT PRIMARY KEY, name TEXT);
pingdart> INSERT INTO users VALUES (1, 'Ravi');
pingdart> SELECT * FROM users;
+----+------+
| id | name |
+----+------+
| 1  | Ravi |
+----+------+
1 row in set

pingdart> exit
Bye
```

---

## ⚙️ Service Administration

Manage PingDartDB using standard Linux systemd commands:

| Action | Command |
| :--- | :--- |
| **Start Service** | `sudo systemctl start pingdartdb` |
| **Stop Service** | `sudo systemctl stop pingdartdb` |
| **Restart Service** | `sudo systemctl restart pingdartdb` |
| **Check Status** | `sudo systemctl status pingdartdb` |
| **Follow Daemon Logs** | `sudo journalctl -u pingdartdb -f` |

---

## 🔄 Upgrades & Safe Rollback

To update PingDartDB safely:

```bash
cd /path/to/pingdartdb
git pull origin main
sudo ./upgrade.sh
```

**Safeguards Built-In**:
- Pauses the service and executes a filesystem `sync` to guarantee an **ACID-consistent snapshot** before touching active files.
- Preserves pre-upgrade binaries as `.rollback`.
- Automatically executes post-upgrade health checks.
- If health verification fails:
  - Preserves the failed state into `/var/lib/pingdartdb/data_failed_upgrade_<TIMESTAMP>` for forensics.
  - Automatically restores previous binaries and restores the database catalog from the verified snapshot.

---

## 🗑️ Safe Uninstallation

```bash
sudo ./uninstall.sh
```
- Stops and disables `pingdartdb.service`.
- Removes systemd units, logrotate configs, and `/usr/local/bin` binaries.
- **Retains all database data and configuration by default.**
- To purge all data permanently, pass `--purge` or confirm `DELETE` interactively.

---

## 🔒 Security Hardening

- **Systemd Sandboxing**: `ProtectSystem=strict`, `ProtectHome=true`, `PrivateTmp=true`, `NoNewPrivileges=true`.
- **Credential Protection**: Passwords are never logged in plaintext or exposed in `/proc` process listings.
- **Network Binding**: Defaults to `127.0.0.1:4650` for local security. Remote cluster mesh is configured explicitly via `/etc/pingdartdb/config.toml`.
