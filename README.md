# PingDartDB

**PingDartDB** is an enterprise AI-native, multi-model distributed database engine built for high-throughput OLTP, columnar OLAP analytics, HNSW vector search, Raft replication, and MySQL-style SQL administration with a modern phpMyAdmin-style Studio.

---

## ⚡ Quick Start: How to Run in Terminal

### 📋 Prerequisites
- **Rust Toolchain (Cargo 1.80+)**:
  - **Windows**: Run in PowerShell: `winget install Rustlang.Rustup` or download installer from [rustup.rs](https://rustup.
  rs).
  - **Linux / macOS**: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Node.js (v20+) & npm** (for Next.js phpMyAdmin Studio):
  - **Windows**: `winget install OpenJS.NodeJS` or `nvm install 22`
  - **Linux / macOS**: `nvm install 22`

---

### 🖥️ Option 1: One-Click Scripts (Windows)

#### Start the Database Core Engine (Port 4650):
```cmd
cd Database
run_db.bat
```
*This launches `pingdart-core.exe` on port `4650` and automatically opens the built-in Studio at `http://localhost:4650/database` in your default browser.*

#### Start Entire Cloud Platform (Database + HBSE Storage + Studio):
```cmd
run_realtime_cloud.bat
```
*Launches the Database Core (`:4650`), HBSE Content-Addressed Storage (`:3221`), and Next.js Studio (`:3000`).*

---

### 💻 Option 2: Running Directly via Terminal Commands

#### Step 1: Start the Database Core Engine
Open your terminal (PowerShell, CMD, or Bash):
```bash
# Navigate to the Database directory
cd "e:\Ravindra Projucts\PingDartRealTimeAPIS\Database"

# Compile and start the engine on 0.0.0.0:4650
cargo run --bin pingdart-core
```
Output:
```text
[INFO] PingDart Core Database Engine starting on 0.0.0.0:4650
[INFO] Sled Storage Trees mounted in ./pingdart_data
[INFO] Root Administrator 'ravi80181' initialized (Roles: MainAdmin)
[INFO] Realtime CDC WebSocket listening on ws://0.0.0.0:4650/v1/realtime
```

#### Step 2: Start the Next.js phpMyAdmin Studio UI
In a second terminal window:
```bash
# Navigate to the studio directory
cd "e:\Ravindra Projucts\PingDartRealTimeAPIS\Database\studio"

# Install dependencies (first time only)
npm install

# Start the Next.js development server
npm run dev
```
Open **[http://localhost:3000](http://localhost:3000)** in your browser to access the Studio.

> [!TIP]
> **Production Static Serving**: You can also compile the Next.js studio statically into the Rust binary:
> ```bash
> cd Database/studio
> npm run build
> ```
> Once built, the Rust engine serves the Studio directly at **[http://localhost:4650/database](http://localhost:4650/database)** without needing Node.js running!

#### Step 3: Run the Terminal CLI SQL Shell
In a third terminal window:
```bash
cd "e:\Ravindra Projucts\PingDartRealTimeAPIS\Database"

# Launch interactive MySQL-style REPL
cargo run --bin cli
```
Or execute a single SQL query directly from terminal:
```bash
cargo run --bin cli -- -e "SHOW DATABASES;"
cargo run --bin cli -- -e "SHOW USERS;"
cargo run --bin cli -- -e "SELECT * FROM messages LIMIT 10;"
```

---

## 🔑 Default Administrator Credentials

| Role | Username | Password | Security PIN | Access Scope |
| :--- | :--- | :--- | :--- | :--- |
| **Main Root Admin** | `ravi80181` | `RavindraChinnu12#` | `80181` | **All databases & user management** |
| **Admin Alias** | `admin` | `admin123` | `80181` | **All databases & user management** |
| **Standard Users** | *Created by Admin* | *Custom* | — | **Scoped strictly to assigned DBs** |

> [!NOTE]
> In the Studio UI login screen, click **"One-Click Login as Root Admin"** to automatically authenticate with `ravi80181`.

---

## 🌐 Network Ports & Endpoints

| Service | Port | Protocol | Purpose / URL |
| :--- | :--- | :--- | :--- |
| **Database Core Engine** | `4650` | HTTP / REST | `http://localhost:4650` |
| **Built-in Studio UI** | `4650` | HTTP | `http://localhost:4650/database` |
| **Live CDC WebSocket** | `4650` | WebSocket | `ws://localhost:4650/v1/realtime` |
| **Next.js Dev Studio** | `3000` | HTTP | `http://localhost:3000` |
| **HBSE Storage Engine** | `3221` | HTTP / REST | `http://localhost:3221` |

---

## 🧪 Testing REST Endpoints via cURL & PowerShell

### 1. Health Status Check
**Terminal (Bash / CMD):**
```bash
curl http://localhost:4650/
```
**PowerShell:**
```powershell
Invoke-RestMethod -Uri "http://localhost:4650/" -Method Get
```

### 2. User Authentication (Login API)
**cURL:**
```bash
curl -X POST http://localhost:4650/login \
     -H "Content-Type: application/json" \
     -d '{"username":"ravi80181","password":"RavindraChinnu12#"}'
```
Response:
```json
{
  "status": "success",
  "token": "4a7c8b21-93e1-4c28-9842-e19efc23d04a"
}
```

### 3. Execute Raw SQL Query
**cURL:**
```bash
curl -X POST http://localhost:4650/v1/query \
     -H "Content-Type: application/json" \
     -H "X-PingDart-Project: default" \
     -d '{"query":"SHOW TABLES;","database":"default"}'
```

### 4. Fetch Records from a Table
**cURL:**
```bash
curl "http://localhost:4650/v1/data/messages?limit=10&order=id.desc" \
     -H "X-PingDart-Project: default"
```

---

## 🚀 Linux Service Installation

### Standard Git Install (Linux Production)

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

## ⚙️ Service Administration (Linux Systemd)

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
