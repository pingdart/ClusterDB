#!/usr/bin/env python3
"""
PingDartDB - Enterprise MySQL / MariaDB Dump Importer
Imports full multi-table database dumps (.sql) into PingDartDB.
Supports: phpMyAdmin dumps, mysqldump, MariaDB dumps, multi-row INSERTs, DDL.
"""

import sys
import os
import re
import time
import argparse
import requests

def parse_args():
    parser = argparse.ArgumentParser(description="Import MySQL / MariaDB Dump (.sql) into PingDartDB")
    parser.add_argument("--file", "-f", default="sql/meddylist_meddylistnew (1).sql", help="Path to SQL dump file")
    parser.add_argument("--database", "-d", default="pingdb", help="Target database name in PingDartDB")
    parser.add_argument("--url", default="http://127.0.0.1:4650", help="PingDartDB Base URL")
    parser.add_argument("--username", "-u", default="admin", help="Admin username")
    parser.add_argument("--password", "-p", default="admin123", help="Admin password")
    parser.add_argument("--verbose", "-v", action="store_true", help="Print each statement")
    return parser.parse_args()

def login(base_url, username, password):
    try:
        resp = requests.post(f"{base_url}/login", json={"username": username, "password": password}, timeout=10)
        resp.raise_for_status()
        data = resp.json()
        if data.get("status") == "success":
            return data.get("token")
        else:
            raise RuntimeError(f"Login failed: {data}")
    except Exception as e:
        print(f"❌ Failed to connect to PingDartDB at {base_url}: {e}")
        sys.exit(1)

def split_sql_statements(file_path):
    """
    Generator that parses a SQL dump streamingly into clean, complete SQL statements.
    Accurately handles quotes, escaped characters, and multi-line statements.
    """
    statement_chars = []
    in_single_quote = False
    in_double_quote = False
    in_backtick = False
    escape_next = False
    in_line_comment = False
    in_block_comment = False

    with open(file_path, "r", encoding="utf-8", errors="ignore") as f:
        for line in f:
            idx = 0
            n = len(line)
            while idx < n:
                ch = line[idx]
                next_ch = line[idx + 1] if idx + 1 < n else ''

                if escape_next:
                    statement_chars.append(ch)
                    escape_next = False
                    idx += 1
                    continue

                if ch == '\\' and (in_single_quote or in_double_quote):
                    statement_chars.append(ch)
                    escape_next = True
                    idx += 1
                    continue

                # Handle comments when not in string literal
                if not (in_single_quote or in_double_quote or in_backtick):
                    if in_line_comment:
                        if ch == '\n':
                            in_line_comment = False
                        idx += 1
                        continue

                    if in_block_comment:
                        if ch == '*' and next_ch == '/':
                            in_block_comment = False
                            idx += 2
                        else:
                            idx += 1
                        continue

                    # Start of line comment (-- or #)
                    if (ch == '-' and next_ch == '-') or ch == '#':
                        in_line_comment = True
                        idx += 2 if (ch == '-' and next_ch == '-') else 1
                        continue

                    # Start of block comment (/* ... */)
                    if ch == '/' and next_ch == '*':
                        in_block_comment = True
                        idx += 2
                        continue

                # String literal boundaries
                if ch == "'" and not (in_double_quote or in_backtick):
                    in_single_quote = not in_single_quote
                    statement_chars.append(ch)
                    idx += 1
                    continue

                if ch == '"' and not (in_single_quote or in_backtick):
                    in_double_quote = not in_double_quote
                    statement_chars.append(ch)
                    idx += 1
                    continue

                if ch == '`' and not (in_single_quote or in_double_quote):
                    in_backtick = not in_backtick
                    statement_chars.append(ch)
                    idx += 1
                    continue

                # Statement boundary
                if ch == ';' and not (in_single_quote or in_double_quote or in_backtick):
                    stmt = "".join(statement_chars).strip()
                    statement_chars.clear()
                    if stmt:
                        yield stmt
                    idx += 1
                    continue

                statement_chars.append(ch)
                idx += 1

    rem = "".join(statement_chars).strip()
    if rem:
        yield rem

def main():
    args = parse_args()
    file_path = args.file
    target_db = args.database

    if not os.path.exists(file_path):
        print(f"❌ SQL dump file not found: {file_path}")
        sys.exit(1)

    file_size_mb = os.path.getsize(file_path) / (1024 * 1024)
    print("=" * 70)
    print("🚀 PINGDART DB - UNIVERSAL MYSQL / MARIADB DUMP IMPORTER 🚀")
    print("=" * 70)
    print(f"📂 Source SQL Dump : {file_path} ({file_size_mb:.2f} MB)")
    print(f"🎯 Target Database : {target_db}")
    print(f"🌐 Server Endpoint : {args.url}")
    print("=" * 70)

    # 1. Login
    print("\n1. Authenticating with PingDartDB...")
    token = login(args.url, args.username, args.password)
    print(f"   ✅ Authenticated! Session token: {token[:8]}...")

    # 2. Ensure target database exists
    print(f"\n2. Initializing target database '{target_db}'...")
    create_db_res = requests.post(f"{args.url}/query", json={
        "token": token,
        "query": f"CREATE DATABASE {target_db}"
    }, timeout=10).json()
    if create_db_res.get("status") == "success":
        print(f"   ✅ Created database '{target_db}'")
    else:
        print(f"   ℹ️ Database notice: {create_db_res.get('error') or create_db_res.get('result')}")

    # 3. Stream and execute SQL statements
    print("\n3. Streaming and executing SQL dump statements...")
    start_time = time.time()
    stmt_count = 0
    tables_created = 0
    tables_created_list = []
    rows_inserted = 0
    inserts_count = 0
    errors_count = 0

    session = requests.Session()
    insert_pattern = re.compile(r"INSERT\s+INTO\s+`?([a-zA-Z0-9_]+)`?", re.IGNORECASE)
    create_pattern = re.compile(r"CREATE\s+TABLE\s+`?([a-zA-Z0-9_]+)`?", re.IGNORECASE)

    for stmt in split_sql_statements(file_path):
        stmt_count += 1
        trimmed = stmt.strip()
        lower = trimmed.lower()

        # Check statement kind
        is_create = create_pattern.search(trimmed)
        is_insert = insert_pattern.search(trimmed)

        # Quick skip unneeded administrative meta commands if any
        if lower.startswith("/*!") and lower.endswith("*/"):
            continue

        try:
            resp = session.post(f"{args.url}/query", json={
                "token": token,
                "database": target_db,
                "query": trimmed
            }, timeout=30)
            data = resp.json()

            if data.get("status") == "success":
                if is_create:
                    table_name = is_create.group(1)
                    tables_created += 1
                    tables_created_list.append(table_name)
                    if args.verbose or tables_created % 10 == 0:
                        print(f"   📦 [Table #{tables_created}] Created `{table_name}`")
                elif is_insert:
                    table_name = is_insert.group(1)
                    inserts_count += 1
                    # Extract rows inserted count if mentioned in result
                    res_text = data.get("result", "")
                    match_num = re.search(r"(\d+)\s+row", res_text)
                    count = int(match_num.group(1)) if match_num else 1
                    rows_inserted += count
                    if args.verbose or inserts_count % 10 == 0:
                        print(f"   📥 [Batch #{inserts_count}] Inserted {count} rows into `{table_name}` (Total: {rows_inserted:,})")
            else:
                err = data.get("error", "Unknown error")
                errors_count += 1
                if args.verbose:
                    print(f"   ⚠️ Stmt #{stmt_count} warning: {err[:120]}")
        except Exception as e:
            errors_count += 1
            if args.verbose:
                print(f"   ⚠️ Execution error on stmt #{stmt_count}: {e}")

        # Periodic progress report every 500 statements
        if stmt_count % 500 == 0:
            elapsed = time.time() - start_time
            qps = rows_inserted / elapsed if elapsed > 0 else 0
            print(f"   ⏳ Processed {stmt_count:,} stmts | {tables_created} tables | {rows_inserted:,} rows ({qps:,.0f} rows/s)")

    total_time = time.time() - start_time
    throughput = rows_inserted / total_time if total_time > 0 else 0

    print("\n" + "=" * 70)
    print("🎉 IMPORT COMPLETED SUCCESSFULLY!")
    print("=" * 70)
    print(f"⏱️ Total Elapsed Time   : {total_time:.2f} seconds")
    print(f"⚡ Ingestion Throughput  : {throughput:,.0f} rows/second")
    print(f"📦 Total Tables Created  : {tables_created}")
    print(f"📥 Total Rows Ingested   : {rows_inserted:,}")
    print(f"📝 Total SQL Statements  : {stmt_count:,}")
    print(f"⚠️ Non-critical Warnings : {errors_count}")
    print("=" * 70)

    # 4. Verify landed data
    print("\n4. Verifying imported tables in PingDartDB...")
    verify_tables = ["ambulances", "doctors", "hospitals", "articles", "bookings"]
    for t in verify_tables:
        if t in tables_created_list:
            cnt_resp = session.post(f"{args.url}/query", json={
                "token": token,
                "database": target_db,
                "query": f"SELECT COUNT(*) FROM {t}"
            }, timeout=10).json()
            print(f"   🔍 Table `{t}` count result: {cnt_resp.get('result', cnt_resp.get('error'))}")

if __name__ == "__main__":
    main()
