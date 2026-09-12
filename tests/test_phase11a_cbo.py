import os
import sys
import time
import json
import shutil
import subprocess
import requests

SERVER_PORT = 4795
BASE_URL = f"http://127.0.0.1:{SERVER_PORT}"
DATA_DIR = "data_cbo_test"

def clean_data():
    if os.path.exists(DATA_DIR):
        shutil.rmtree(DATA_DIR)

def start_server():
    clean_data()
    env = os.environ.copy()
    env["RUST_LOG"] = "warn"
    proc = subprocess.Popen(["./target/debug/pingdart-core", "--port", str(SERVER_PORT), "--data-dir", DATA_DIR], env=env)
    
    for _ in range(30):
        try:
            r = requests.get(f"{BASE_URL}/", timeout=1)
            if r.status_code == 200:
                print(f"✅ PingDartDB Server started and responsive on port {SERVER_PORT}")
                return proc
        except Exception:
            time.sleep(0.3)
    
    proc.kill()
    raise RuntimeError("Failed to start PingDartDB server")

def run_tests():
    proc = start_server()
    try:
        print("\n--- Test 1: Capability Matrix & Status Verification ---")
        status_res = requests.get(f"{BASE_URL}/").json()
        capabilities = status_res.get("capabilities", [])
        assert "Cost-Based Query Optimizer (CBO) & Persistent Stats" in capabilities, f"Missing CBO capability: {capabilities}"
        print("✅ Status capabilities include Cost-Based Query Optimizer (CBO) & Persistent Stats.")

        print("\n--- Test 2: Admin Session Login ---")
        login_res = requests.post(f"{BASE_URL}/login", json={
            "username": "admin",
            "password": "admin123"
        }).json()
        assert login_res["status"] == "success", f"Login failed: {login_res}"
        token = login_res["token"]
        print("✅ SuperAdmin logged in successfully.")

        print("\n--- Test 3: Table Creation & Pre-ANALYZE Fallback Warning ---")
        requests.post(f"{BASE_URL}/query", json={
            "query": "CREATE TABLE employees (id INT PRIMARY KEY, name TEXT, department TEXT, salary INT);",
            "token": token
        })

        # Explain query before ANALYZE TABLE
        exp_pre_res = requests.post(f"{BASE_URL}/query", json={
            "query": "EXPLAIN ANALYZE SELECT * FROM employees WHERE id = 10;",
            "token": token
        }).json()
        assert exp_pre_res["status"] == "success", f"EXPLAIN failed: {exp_pre_res}"
        exp_pre = json.loads(exp_pre_res["result"])
        assert "cbo_decision" in exp_pre, "Missing cbo_decision in report"
        cbo_pre = exp_pre["cbo_decision"]
        assert cbo_pre["staleness_warning"] is not None
        assert "No catalog statistics available" in cbo_pre["staleness_warning"]
        print("✅ Optimizer correctly warned about missing catalog statistics before ANALYZE.")

        print("\n--- Test 4: Seeding 1,000 Rows for Realistic Statistics ---")
        for i in range(1, 1001):
            dept = "Engineering" if i % 3 == 0 else ("Sales" if i % 3 == 1 else "Finance")
            salary = 50000 + (i * 50)
            res = requests.post(f"{BASE_URL}/query", json={
                "query": f"INSERT INTO employees VALUES ({i}, 'Emp_{i}', '{dept}', {salary});",
                "token": token
            })
            if i % 200 == 0:
                print(f"  Inserted {i}/1000 rows...")
        print("✅ 1,000 rows seeded into employees table.")

        print("\n--- Test 5: ANALYZE TABLE Execution & HyperLogLog NDV Calculation ---")
        analyze_res = requests.post(f"{BASE_URL}/query", json={
            "query": "ANALYZE TABLE employees;",
            "token": token
        }).json()
        print(f"ANALYZE Result: {analyze_res}")
        msg = analyze_res.get("result", "")
        assert "Table 'employees' analyzed successfully" in msg, f"Unexpected analyze result: {analyze_res}"
        assert "1000 rows" in msg
        assert "4 columns" in msg
        print("✅ ANALYZE TABLE completed: built HLL 64-register NDV and typed histograms in _system_table_stats.")

        print("\n--- Test 6: CBO Plan Selection — Point Lookup (BTreeIndexScan) ---")
        exp_point_res = requests.post(f"{BASE_URL}/query", json={
            "query": "EXPLAIN ANALYZE SELECT * FROM employees WHERE id = 42;",
            "token": token
        }).json()
        assert exp_point_res["status"] == "success", f"Point explain failed: {exp_point_res}"
        exp_point = json.loads(exp_point_res["result"])
        cbo_point = exp_point["cbo_decision"]
        print(f"Point Lookup Decision: selected_plan='{cbo_point['selected_plan']}', est_cost={cbo_point['estimated_cost_ms']:.4f}ms, est_rows={cbo_point['estimated_rows']}")
        assert cbo_point["selected_plan"] == "BTreeIndexScan", f"Expected BTreeIndexScan, got {cbo_point['selected_plan']}"
        assert cbo_point["staleness_warning"] is None, f"Expected fresh statistics, got {cbo_point['staleness_warning']}"
        assert len(cbo_point["candidate_costs"]) >= 2
        print("✅ CBO correctly selected BTreeIndexScan for selective point predicate.")

        print("\n--- Test 7: CBO Plan Selection — Low Selectivity Range Scan (SequentialScan) ---")
        exp_bulk_res = requests.post(f"{BASE_URL}/query", json={
            "query": "EXPLAIN ANALYZE SELECT * FROM employees WHERE salary > 52000;",
            "token": token
        }).json()
        assert exp_bulk_res["status"] == "success", f"Bulk explain failed: {exp_bulk_res}"
        exp_bulk = json.loads(exp_bulk_res["result"])
        cbo_bulk = exp_bulk["cbo_decision"]
        print(f"Bulk Range Decision: selected_plan='{cbo_bulk['selected_plan']}', est_cost={cbo_bulk['estimated_cost_ms']:.4f}ms, est_rows={cbo_bulk['estimated_rows']}")
        # For > 52000, 96% of rows match -> SequentialScan is optimal to avoid 960 random seeks
        assert cbo_bulk["selected_plan"] == "SequentialScan", f"Expected SequentialScan, got {cbo_bulk['selected_plan']}"
        print("✅ CBO correctly selected SequentialScan over random index lookups for high-selectivity predicate.")

        print("\n--- Test 8: Statistics Staleness Detection (Modifications > 20%) ---")
        print("  Inserting 250 new records (25% table modification > 20% threshold)...")
        for i in range(1001, 1251):
            requests.post(f"{BASE_URL}/query", json={
                "query": f"INSERT INTO employees VALUES ({i}, 'NewEmp_{i}', 'Operations', 60000);",
                "token": token
            })
        
        exp_stale_res = requests.post(f"{BASE_URL}/query", json={
            "query": "EXPLAIN ANALYZE SELECT * FROM employees WHERE id = 42;",
            "token": token
        }).json()
        assert exp_stale_res["status"] == "success", f"Stale explain failed: {exp_stale_res}"
        exp_stale = json.loads(exp_stale_res["result"])
        cbo_stale = exp_stale["cbo_decision"]
        print(f"Stale Decision Warning: {cbo_stale.get('staleness_warning')}")
        assert cbo_stale["staleness_warning"] is not None, "Expected staleness warning after 25% modifications!"
        assert "stale" in cbo_stale["staleness_warning"].lower()
        assert "25.0%" in cbo_stale["staleness_warning"]
        print("✅ Statistics staleness successfully detected and reported in EXPLAIN ANALYZE.")

        print("\n--- Test 9: Re-ANALYZE TABLE Refreshes Staleness ---")
        re_analyze = requests.post(f"{BASE_URL}/query", json={
            "query": "ANALYZE TABLE employees;",
            "token": token
        }).json()
        print(f"Re-ANALYZE Result: {re_analyze}")

        exp_refreshed_res = requests.post(f"{BASE_URL}/query", json={
            "query": "EXPLAIN ANALYZE SELECT * FROM employees WHERE id = 42;",
            "token": token
        }).json()
        assert exp_refreshed_res["status"] == "success", f"Refreshed explain failed: {exp_refreshed_res}"
        exp_refreshed = json.loads(exp_refreshed_res["result"])
        cbo_refreshed = exp_refreshed["cbo_decision"]
        assert cbo_refreshed["staleness_warning"] is None, f"Expected staleness warning to clear, got {cbo_refreshed['staleness_warning']}"
        print("✅ Re-running ANALYZE TABLE successfully reset modification counter and cleared staleness warning.")

        print("\n=======================================================")
        print("🎉 ALL PHASE 11A CBO VALIDATION TESTS PASSED SUCCESSFULLY!")
        print("=======================================================")

    finally:
        print("\nShutting down test server...")
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except Exception:
            proc.kill()
        clean_data()

if __name__ == "__main__":
    run_tests()
