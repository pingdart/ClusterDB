import os
import sys
import time
import json
import shutil
import subprocess
import requests

SERVER_PORT = 4680
BASE_URL = f"http://127.0.0.1:{SERVER_PORT}"
DATA_DIR = "data_enterprise_test"
BACKUP_DIR = f"pingdart_backups_{SERVER_PORT}"

def clean_data():
    if os.path.exists(DATA_DIR):
        shutil.rmtree(DATA_DIR)
    if os.path.exists(BACKUP_DIR):
        shutil.rmtree(BACKUP_DIR)

def start_server():
    clean_data()
    env = os.environ.copy()
    env["RUST_LOG"] = "warn"
    proc = subprocess.Popen(["./target/debug/pingdart-core", "--port", str(SERVER_PORT), "--data-dir", DATA_DIR], env=env)
    
    for _ in range(30):
        try:
            r = requests.get(f"{BASE_URL}/", timeout=1)
            if r.status_code == 200:
                print(f" PingDartDB Server started and responsive on port {SERVER_PORT}")
                return proc
        except Exception:
            time.sleep(0.3)
    
    proc.kill()
    raise RuntimeError("Failed to start PingDartDB server")

def run_tests():
    proc = start_server()
    try:
        # 1. Login
        print("\n--- Test 1: Authentication ---")
        login_res = requests.post(f"{BASE_URL}/login", json={
            "username": "admin",
            "password": "admin123"
        }).json()
        assert login_res["status"] == "success", f"Login failed: {login_res}"
        token = login_res["token"]
        print(" Logged in successfully. Token acquired.")

        # Helper for queries
        def query(sql):
            res = requests.post(f"{BASE_URL}/query", json={"query": sql, "token": token}).json()
            assert res.get("status") == "success", f"Query failed: {sql} -> {res}"
            r = res.get("result")
            if isinstance(r, str):
                try:
                    return json.loads(r)
                except Exception:
                    return r
            return r

        # 2. Vectorized Columnar OLAP Engine
        print("\n--- Test 2: Vectorized Columnar Analytics (OLAP) ---")
        query("CREATE TABLE sales (id INT PRIMARY KEY, category TEXT, amount INT, active BOOL);")
        
        # Insert 100 rows across categories
        print("Inserting 100 analytical rows across Electronics, Books, Fashion...")
        for i in range(1, 101):
            cat = "Electronics" if i % 3 == 0 else ("Books" if i % 3 == 1 else "Fashion")
            amt = i * 10
            is_active = "TRUE" if i % 2 == 0 else "FALSE"
            query(f"INSERT INTO sales (id, category, amount, active) VALUES ({i}, '{cat}', {amt}, {is_active});")
        
        # Test 2.1: Basic Vectorized Aggregation
        agg_result = query("SELECT COUNT(*), SUM(amount), AVG(amount), MIN(amount), MAX(amount) FROM sales;")
        print(f"Overall aggregation result: {agg_result}")
        assert len(agg_result) == 1, f"Expected 1 row, got {agg_result}"
        row = agg_result[0]
        # Sum of 10..1000 = 10 * (100 * 101 / 2) = 50500
        count_val = int(row.get("count") or row.get("COUNT(*)"))
        sum_val = float(row.get("sum_amount") or row.get("SUM(amount)"))
        avg_val = float(row.get("avg_amount") or row.get("AVG(amount)"))
        min_val = float(row.get("min_amount") or row.get("MIN(amount)"))
        max_val = float(row.get("max_amount") or row.get("MAX(amount)"))

        assert count_val == 100, f"Expected 100, got {count_val}"
        assert sum_val == 50500.0, f"Expected 50500, got {sum_val}"
        assert avg_val == 505.0, f"Expected 505.0, got {avg_val}"
        assert min_val == 10.0, f"Expected 10, got {min_val}"
        assert max_val == 1000.0, f"Expected 1000, got {max_val}"
        print(" Global Vectorized Aggregations (COUNT, SUM, AVG, MIN, MAX) verified.")

        # Test 2.2: Vectorized GROUP BY Aggregation
        group_result = query("SELECT category, COUNT(*), SUM(amount), AVG(amount) FROM sales GROUP BY category;")
        print(f"Group By aggregation result: {group_result}")
        assert len(group_result) == 3, f"Expected 3 categories, got {len(group_result)}"
        cat_map = {r["category"]: r for r in group_result}
        assert "Electronics" in cat_map and "Books" in cat_map and "Fashion" in cat_map
        total_count = sum(int(r.get("count") or r.get("COUNT(*)")) for r in group_result)
        total_sum = sum(float(r.get("sum_amount") or r.get("SUM(amount)")) for r in group_result)
        assert total_count == 100, f"Grouped count mismatch: {total_count}"
        assert total_sum == 50500.0, f"Grouped sum mismatch: {total_sum}"
        print(" Vectorized GROUP BY Hash Aggregation verified.")

        # 3. Observability & Kubernetes Probes
        print("\n--- Test 3: Enterprise Observability & K8s Probes ---")
        # 3.1: Liveness Probe
        live_res = requests.get(f"{BASE_URL}/health/live")
        assert live_res.status_code == 200, f"Liveness probe failed: {live_res.status_code}"
        assert live_res.json().get("status") in ["UP", "alive"], f"Unexpected live status: {live_res.json()}"
        print(" Kubernetes Liveness Probe /health/live passed (200 OK).")

        # 3.2: Readiness Probe
        ready_res = requests.get(f"{BASE_URL}/health/ready")
        assert ready_res.status_code == 200, f"Readiness probe failed: {ready_res.status_code}"
        assert ready_res.json().get("status") in ["READY", "ready"], f"Unexpected ready status: {ready_res.json()}"
        print(" Kubernetes Readiness Probe /health/ready passed (200 OK).")

        # 3.3: Prometheus Metrics Exposition
        metrics_res = requests.get(f"{BASE_URL}/metrics")
        assert metrics_res.status_code == 200, f"Metrics endpoint failed: {metrics_res.status_code}"
        metrics_text = metrics_res.text
        assert "pingdart_queries_total" in metrics_text, f"Missing pingdart_queries_total: {metrics_text}"
        assert "pingdart_query_duration_ms_avg" in metrics_text, f"Missing duration: {metrics_text}"
        print(" Prometheus Metrics /metrics exposition verified:")
        for line in metrics_text.strip().split("\n"):
            if not line.startswith("#"):
                print(f"   {line}")

        # 4. Zero-Downtime Backup & Point-In-Time Recovery (PITR)
        print("\n--- Test 4: Online Backup Snapshot & PITR ---")
        # 4.1: Take consistent snapshot
        snap_res = requests.post(f"{BASE_URL}/backup/snapshot", json={"token": token}).json()
        assert snap_res.get("status") == "success", f"Snapshot failed: {snap_res}"
        snap_info = json.loads(snap_res["result"]) if isinstance(snap_res.get("result"), str) else snap_res.get("result", {})
        snapshot_id = snap_info["snapshot_id"]
        snapshot_ts = snap_info["timestamp"]
        print(f" Consistent snapshot created: ID={snapshot_id}, timestamp={snapshot_ts}, rows={snap_info['total_rows']}")

        # 4.2: Perform mutations after snapshot
        print("Inserting post-snapshot rows 101 and 102...")
        query("INSERT INTO sales (id, category, amount, active) VALUES (101, 'Electronics', 5000, TRUE);")
        query("INSERT INTO sales (id, category, amount, active) VALUES (102, 'Electronics', 6000, TRUE);")
        
        count_after = int(query("SELECT COUNT(*) FROM sales;")[0].get("count") or query("SELECT COUNT(*) FROM sales;")[0].get("COUNT(*)"))
        assert count_after == 102, f"Expected 102 rows, got {count_after}"
        print(f" Mutated state verified: row count is now {count_after}.")

        # 4.3: Restore base snapshot -> should revert to 100 rows
        print("Restoring base snapshot...")
        restore_res = requests.post(f"{BASE_URL}/backup/restore", json={
            "token": token,
            "snapshot_id": snapshot_id
        }).json()
        assert restore_res.get("status") == "success", f"Restore failed: {restore_res}"
        
        count_restored = int(query("SELECT COUNT(*) FROM sales;")[0].get("count") or query("SELECT COUNT(*) FROM sales;")[0].get("COUNT(*)"))
        assert count_restored == 100, f"Expected 100 rows after restore, got {count_restored}"
        print(f" Snapshot restored successfully. Row count safely rolled back to {count_restored}.")

        # 4.4: PITR up to current time -> should replay CDC log and bring back rows 101 & 102!
        current_time = int(time.time()) + 5
        print(f"Performing Point-In-Time Recovery to timestamp {current_time}...")
        pitr_res = requests.post(f"{BASE_URL}/backup/pitr", json={
            "token": token,
            "target_timestamp": current_time
        }).json()
        assert pitr_res.get("status") == "success", f"PITR failed: {pitr_res}"
        print(f" PITR response: {pitr_res.get('result')}")

        count_pitr = int(query("SELECT COUNT(*) FROM sales;")[0].get("count") or query("SELECT COUNT(*) FROM sales;")[0].get("COUNT(*)"))
        assert count_pitr == 102, f"Expected 102 rows after PITR replay, got {count_pitr}"
        print(f" PITR verification complete: Row count successfully restored to {count_pitr} via continuous CDC replay!")

        print("\n ALL PHASE 8 ENTERPRISE PLATFORM ARCHITECTURE TESTS PASSED SUCCESSFULLY!")

    finally:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            proc.kill()
        clean_data()

if __name__ == "__main__":
    run_tests()
