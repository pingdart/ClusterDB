# PingDartDB SQL Reference & Command Manual (MySQL Compatible)

PingDartDB implements a high-performance, embedded, distributed storage engine with a **MySQL-compatible SQL interface** powered by `sqlparser (MySqlDialect)`.

This document serves as the complete operational cheat-sheet and manual for database administrators, developers, and autonomous agents.

---

## Table of Contents
1. [User Management & Security](#1-user-management--security)
2. [Database Operations](#2-database-operations)
3. [Table Definition (DDL)](#3-table-definition-ddl)
4. [Data Manipulation (DML)](#4-data-manipulation-dml)
5. [Indexing & Query Optimization](#5-indexing--query-optimization)
6. [Multi-Tenancy & Row-Level Security (RLS)](#6-multi-tenancy--row-level-security-rls)
7. [Enterprise Backup, Snapshot & Restore (.pb)](#7-enterprise-backup-snapshot--restore-pb)
8. [Universal Migration Framework](#8-universal-migration-framework)
9. [MySQL Compatibility Cheat Sheet](#9-mysql-compatibility-cheat-sheet)

---

## 1. User Management & Security

PingDartDB isolates authentication and credentials in an internal high-performance key-value tree (`_users`). Native DDL statements are used to provision, modify, and revoke access, including enterprise IP- and host-based access control.

### A. Create User (Host- & IP-Based Access Control)
PingDart supports both **MySQL-style `'username'@'host'`** syntax and explicit **`IP '<pattern>'`** clauses.

```sql
-- 1. MySQL-style 'user'@'host' syntax
CREATE USER 'developer'@'localhost' PASSWORD 'Secret123!';
CREATE USER 'backend'@'192.168.1.%' PASSWORD 'Pass456!';
CREATE USER 'global_admin'@'%' PASSWORD 'AdminMasterPass!';

-- 2. Explicit IP / HOST syntax
CREATE USER developer PASSWORD 'Secret123!' IP '192.168.1.50';
CREATE USER analytics_svc PASSWORD 'AppPass99$' IP '10.0.0.%';

-- 3. Multi-Host / Comma-Separated Whitelist
CREATE USER microservice PASSWORD 'ClusterSecret!' IP '127.0.0.1, 192.168.1.%, 10.0.0.10';
```

#### Supported Host & IP Patterns:
| Host Pattern | Example | Meaning |
| :--- | :--- | :--- |
| `%` or `*` | `'app'@'%'` or `IP '%'` | **Any host/IP allowed** (Default) |
| `localhost` | `'app'@'localhost'` | Only local connections (`127.0.0.1`, `::1`) |
| Exact IP | `IP '192.168.1.45'` | Only connection from `192.168.1.45` |
| Subnet Wildcard | `IP '192.168.1.%'` | Any IP in subnet (e.g. `192.168.1.1` to `192.168.1.254`) |
| Class A/B Wildcard | `IP '10.0.%'` | Any IP starting with `10.0.` |
| Multi-Host List | `IP '127.0.0.1, 10.0.0.%'` | Whitelists localhost and entire 10.0.0.x subnet |

### B. Modify User Host/IP or Password (ALTER USER)
You can update a user's password, their host whitelist, or both simultaneously:
```sql
-- Update password only:
ALTER USER developer PASSWORD 'NewStrongerPassword456!';

-- Update host / IP restriction only:
ALTER USER developer IP '10.0.0.%';
ALTER USER 'developer'@'%' IP '192.168.1.100';

-- Update both password and host restriction:
ALTER USER developer PASSWORD 'RotatedPassword2026!' IP '192.168.1.50';
```

### C. Drop User
Delete a user and revoke all active session tokens. Both standard and `@host` syntax are accepted:
```sql
DROP USER <username>;
DROP USER '<username>'@'<host>';

-- Examples:
DROP USER developer;
DROP USER 'developer'@'localhost';
```

### D. Grant Privileges
Grant role permissions to a user on a designated database catalog:
```sql
GRANT <PRIVILEGE> ON <database> TO <username>;
GRANT <PRIVILEGE> ON <database> TO '<username>'@'<host>';

-- Examples:
GRANT ALL ON default TO developer;
GRANT ALL ON default TO 'developer'@'192.168.1.%';
GRANT READ ON production_db TO reporting_user;
GRANT WRITE ON analytics_db TO etl_worker;
```
*Supported Privileges:* `ALL` / `ALL PRIVILEGES`, `READ` / `SELECT`, `WRITE` / `READWRITE`.

---

## 2. Database Operations

### A. Create Database
Create a new isolated database catalog.
```sql
CREATE DATABASE <dbname>;

-- Example:
CREATE DATABASE ecommerce_prod;
```

### B. Switch Active Database
Switch the current execution context for subsequent table queries.
```sql
USE <dbname>;

-- Example:
USE ecommerce_prod;
```

### C. Show Databases
List all accessible databases in the PingDart instance.
```sql
SHOW DATABASES;
```

### D. Drop Database
Drop a database along with all its tables, indexes, and schemas.
```sql
DROP DATABASE <dbname>;

-- Example:
DROP DATABASE ecommerce_prod;
```

---

## 3. Table Definition (DDL)

### A. Create Table
Define a structured table with typed columns.
```sql
CREATE TABLE <table_name> (
    id INT,
    name VARCHAR(255),
    email VARCHAR(255),
    balance FLOAT,
    is_active BOOLEAN,
    created_at TIMESTAMP
);

-- Example:
CREATE TABLE customers (
    id INT,
    full_name VARCHAR(100),
    email VARCHAR(150),
    status VARCHAR(50),
    credit_limit FLOAT
);
```
*Supported Column Types:* `INT`, `BIGINT`, `FLOAT`, `DOUBLE`, `VARCHAR`, `TEXT`, `BOOLEAN`, `TIMESTAMP`, `JSON`, `VECTOR`.

### B. Show Tables
List all tables within the currently selected database.
```sql
SHOW TABLES;
```

### C. Describe Table
Inspect table column names and inferred schema datatypes.
```sql
DESCRIBE <table_name>;
-- or alias:
DESC <table_name>;

-- Example:
DESCRIBE customers;
```

### D. Analyze Table
Collect table statistics and cardinality histograms for the Cost-Based Optimizer (CBO).
```sql
ANALYZE TABLE <table_name>;

-- Example:
ANALYZE TABLE customers;
```

### E. Drop Table
Drop table and destroy its persistent storage tree.
```sql
DROP TABLE <table_name>;

-- Example:
DROP TABLE customers;
```

---

## 4. Data Manipulation (DML)

### A. Insert Records
Insert single or multiple rows into a table.
```sql
INSERT INTO <table_name> (<columns>) VALUES (<values>);

-- Examples:
INSERT INTO customers (id, full_name, email, status, credit_limit) 
VALUES (1, 'Alice Johnson', 'alice@corp.com', 'active', 5000.00);

INSERT INTO customers (id, full_name, email, status, credit_limit) 
VALUES (2, 'Bob Smith', 'bob@example.com', 'pending', 1200.50);
```

### B. Query Records (SELECT)
Retrieve rows with column projections, filtering, ordering, and pagination.
```sql
-- Select all columns
SELECT * FROM customers;

-- Projection and WHERE filtering
SELECT id, full_name, email FROM customers WHERE status = 'active';

-- Numeric comparison and range filtering
SELECT * FROM customers WHERE credit_limit >= 2000.00;

-- Sorting and pagination
SELECT * FROM customers ORDER BY id DESC LIMIT 10 OFFSET 0;

-- Aggregate Count
SELECT COUNT(*) FROM customers;
```

### C. Update Records
Modify existing column values matching a predicate.
```sql
UPDATE <table_name> SET <column> = <value> WHERE <condition>;

-- Example:
UPDATE customers SET status = 'active', credit_limit = 7500.00 WHERE id = 2;
```

### D. Delete Records
Remove records matching a predicate.
```sql
DELETE FROM <table_name> WHERE <condition>;

-- Example:
DELETE FROM customers WHERE id = 1;
DELETE FROM customers WHERE status = 'pending';
```

---

## 5. Indexing & Query Optimization

PingDart provides high-speed B-Tree secondary indexes and Vector embeddings indexing.

### A. Create Secondary Index
Create a secondary index to accelerate `WHERE` lookups.
```sql
CREATE INDEX <index_name> ON <table_name> (<field1>, <field2>);

-- Example:
CREATE INDEX idx_customers_status ON customers (status);
CREATE INDEX idx_customers_email ON customers (email);
```

### B. Drop Secondary Index
Remove a secondary index.
```sql
DROP INDEX ON <table_name> (<field1>);

-- Example:
DROP INDEX ON customers (status);
```

### C. Show Indexes & Statistics
```sql
-- List existing indexes across tables
SHOW INDEXES;

-- View B-tree depth, lookups count, and hit-rates
SHOW INDEX STATS;
```

### D. EXPLAIN & EXPLAIN ANALYZE
Inspect the query execution plan generated by the Cost-Based Optimizer (CBO).
```sql
-- View plan tree and cost estimates (CollectionScan vs IndexScan)
EXPLAIN SELECT * FROM customers WHERE id = 1;

-- Execute query and measure actual node latencies
EXPLAIN ANALYZE SELECT * FROM customers WHERE status = 'active';
```

---

## 6. Multi-Tenancy & Row-Level Security (RLS)

Implement multi-tenant data isolation and security policies at the SQL engine level.

### A. Create Tenant Policy
Define row-level filtering policies automatically applied to tenant queries.
```sql
CREATE POLICY <policy_name> ON <table_name> FOR <command> USING (<expression>);

-- Example:
CREATE POLICY tenant_isolation_policy ON customers FOR SELECT USING (tenant_id = current_tenant);
```

### B. Show Policies
```sql
SHOW POLICIES;
SHOW POLICIES FOR customers;
```

---

## 7. Enterprise Backup, Snapshot & Restore (.pb)

High-speed streaming backup using encrypted Protocol Buffers packages.

### A. Export Database
```sql
EXPORT DATABASE <dbname> TO '<backup_path>.pb' [ENCRYPTED] [KEY '<secret_passphrase>'];

-- Example:
EXPORT DATABASE default TO 'backups/default_2026.pb' ENCRYPTED KEY 'VaultSecretKey99';
```

### B. Import Database
```sql
IMPORT DATABASE FROM '<backup_path>.pb' [KEY '<secret_passphrase>'];

-- Example:
IMPORT DATABASE FROM 'backups/default_2026.pb' KEY 'VaultSecretKey99';
```

### C. Export Entire Cluster
```sql
EXPORT CLUSTER TO 'backups/cluster_full.pb' ENCRYPTED KEY 'EnterpriseAdminKey';
IMPORT CLUSTER FROM 'backups/cluster_full.pb' KEY 'EnterpriseAdminKey';
```

---

## 8. Universal Migration Framework

Direct zero-downtime data streaming from external database engines into PingDart.

### A. Start Migration Job
```sql
MIGRATE FROM POSTGRESQL SOURCE 'postgres://user:pass@host:5432/db' TO local_table;
MIGRATE FROM MONGODB SOURCE 'mongodb://localhost:27017/analytics' TO local_docs;
MIGRATE FROM CSV SOURCE 'data/imports/customers.csv' TO customers;
```

### B. Monitor & Control Migrations
```sql
SHOW MIGRATIONS;
SHOW MIGRATION <job_id>;
RESUME MIGRATION <job_id>;
ABORT MIGRATION <job_id>;
VALIDATE MIGRATION <job_id>;
```

---

## 9. MySQL Compatibility Cheat Sheet

| Task | Standard MySQL Syntax | PingDartDB Syntax | Supported? |
| :--- | :--- | :--- | :---: |
| **Create User** | `CREATE USER 'user'@'%' IDENTIFIED BY 'pass';` | `CREATE USER user PASSWORD pass [IP %];` | ✅ **YES** |
| **Change Password** | `ALTER USER 'user'@'%' IDENTIFIED BY 'new';` | `ALTER USER user PASSWORD new;` | ✅ **YES** |
| **Drop User** | `DROP USER 'user'@'%';` | `DROP USER user;` | ✅ **YES** |
| **Grant Privileges** | `GRANT ALL ON db.* TO 'user'@'%';` | `GRANT ALL ON db TO user;` | ✅ **YES** |
| **Create DB** | `CREATE DATABASE store;` | `CREATE DATABASE store;` | ✅ **YES** |
| **Show DBs** | `SHOW DATABASES;` | `SHOW DATABASES;` | ✅ **YES** |
| **Select DB** | `USE store;` | `USE store;` | ✅ **YES** |
| **Show Tables** | `SHOW TABLES;` | `SHOW TABLES;` | ✅ **YES** |
| **Describe** | `DESCRIBE users;` | `DESCRIBE users;` | ✅ **YES** |
| **Insert** | `INSERT INTO tbl (a, b) VALUES (1, 2);` | `INSERT INTO tbl (a, b) VALUES (1, 2);` | ✅ **YES** |
| **Select** | `SELECT * FROM tbl WHERE id=1 LIMIT 10;` | `SELECT * FROM tbl WHERE id=1 LIMIT 10;` | ✅ **YES** |
| **Update** | `UPDATE tbl SET a=1 WHERE id=2;` | `UPDATE tbl SET a=1 WHERE id=2;` | ✅ **YES** |
| **Delete** | `DELETE FROM tbl WHERE id=2;` | `DELETE FROM tbl WHERE id=2;` | ✅ **YES** |
| **Indexes** | `CREATE INDEX idx ON tbl (col);` | `CREATE INDEX idx ON tbl (col);` | ✅ **YES** |
| **Execution Plan**| `EXPLAIN SELECT ...;` | `EXPLAIN SELECT ...;` | ✅ **YES** |
| **Execution Stats**| `EXPLAIN ANALYZE SELECT ...;` | `EXPLAIN ANALYZE SELECT ...;` | ✅ **YES** |
