# PingDartDB Core - Usage & Developer Guide

This document explains how to start the PingDart unified database engine and interact with it using both traditional SQL and the modern Document API.

## 1. Starting the Database Server

The database engine is built in Rust and runs an extremely fast TCP/HTTP server. 

To start the server, open a terminal in the `pingdart-core` directory and run:

```bash
cargo run
```

When it starts successfully, you will see output like this:
```text
PingDart Core Engine Starting...
Storage engine initialized at ./pingdart_data
Root user initialized with default credentials
Starting PingDart HTTP API on 0.0.0.0:4650
```

The database is now listening for connections on port `4650`.

---

## 2. Authentication

By default, the engine generates a "Main Admin" (Root User) on startup with the following credentials:
- **Username**: `admin`
- **Password**: `admin123`

You must first login to receive a session token.

### Login Example

```bash
curl -X POST http://localhost:4650/login \
     -H "Content-Type: application/json" \
     -d '{"username": "admin", "password": "admin123"}'
```

**Expected Response**:
```json
{
  "status": "success",
  "token": "a1b2c3d4-...",
  "error": null
}
```

Copy the `token` from the response. You will use this token in all subsequent requests.

---

## 3. The Unified Engine: Two Ways to Query

PingDartDB features a **Unified Storage Engine**. This means you can interact with the exact same underlying data using either a JSON Document API or traditional SQL.

### Option A: The Document API (Modern)

The Document API is ideal for modern web and mobile applications. It scales better and avoids complex SQL string construction.

**Endpoint**: `POST /document/query`

**Example: Querying a Collection with Filters**
```bash
curl -X POST http://localhost:4650/document/query \
     -H "Content-Type: application/json" \
     -d '{
       "token": "YOUR_TOKEN_HERE",
       "collection": "users",
       "where": [
           {
               "field": "city",
               "operator": "==",
               "value": "Hyderabad"
           }
       ],
       "orderBy": "name",
       "limit": 20,
       "offset": 0
     }'
```

**Expected Response**:
Returns an array of standard JSON documents, automatically enriched with versioning metadata (`_id`, `_version`, `_createdAt`, etc.).

### Option B: The SQL API (Traditional)

The SQL API is great for traditional DDL operations (creating tables) and complex joins.

**Endpoint**: `POST /query`

**Example 1: Creating a Table with Constraints**
```bash
curl -X POST http://localhost:4650/query \
     -H "Content-Type: application/json" \
     -d '{
       "token": "YOUR_TOKEN_HERE", 
       "query": "CREATE TABLE users (id INT AUTO_INCREMENT PRIMARY KEY, name STRING, created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP)"
     }'
```

**Example 2: Inserting Data**
```bash
curl -X POST http://localhost:4650/query \
     -H "Content-Type: application/json" \
     -d '{
       "token": "YOUR_TOKEN_HERE", 
       "query": "INSERT INTO users (name) VALUES (\"Alice\")"
     }'
```

---

## 4. Administrative Commands

Administrative operations are handled through the SQL API endpoint (`POST /query`).

### Creating a Database
```bash
curl -X POST http://localhost:4650/query \
     -H "Content-Type: application/json" \
     -d '{"token": "YOUR_TOKEN_HERE", "query": "CREATE DATABASE my_app"}'
```

### Creating Users with IP Whitelisting
To create a user who can only connect from a specific IP address:
```bash
curl -X POST http://localhost:4650/query \
     -H "Content-Type: application/json" \
     -d '{"token": "YOUR_TOKEN_HERE", "query": "CREATE USER alice PASSWORD secret IP 192.168.1.100"}'
```
*(Use `IP %` to allow global access)*

### Exporting Database Snapshots
To export your entire database schema and all rows into a custom PingDart structured `.pg` file:
```bash
curl -X POST http://localhost:4650/query \
     -H "Content-Type: application/json" \
     -d '{"token": "YOUR_TOKEN_HERE", "query": "EXPORT DATABASE my_app"}'
```
This automatically extracts the data and writes it to `my_app.pg` on the server!

---

## 5. Node.js SDK Integration (Legacy SQL Style)

If you have a Node.js backend and don't want to write raw HTTP requests, we provide a custom driver that behaves **exactly** like the popular `mysql` node package!

In your `backend/src/` folder, import `pingdart.js`:

```javascript
const pingdart = require('./pingdart');

// 1. Create a connection
const db = pingdart.createConnection({
    host: 'localhost',
    port: 4650,
    user: 'admin',
    password: 'admin123',
    database: 'my_app'
});

// 2. Connect to the engine
db.connect((err) => {
    if (err) throw err;
    console.log("Connected to PingDartDB!");

    // 3. Run queries directly!
    db.query("SELECT * FROM users", (err, results) => {
        if (err) throw err;
        
        // Prints your rows as standard Javascript Arrays/Objects!
        console.log(results); 
    });
});
```

> **Note**: A new Document-oriented SDK method (`db.collection('users').where(...)`) is currently in development to interface natively with the new Document API!

---

## 6. Where is the data stored?

The V1 database engine uses `sled` under the hood. All binary data is stored safely in the `pingdart-core/pingdart_data/` directory. You can stop the server (`Ctrl+C`) and start it again; your collections and tables will persist securely on disk.
