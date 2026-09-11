import os
import sys
import time
import json
import shutil
import subprocess
import requests

SERVER_PORT = 4805
BASE_URL = f"http://127.0.0.1:{SERVER_PORT}"
DATA_DIR = "data_phase13b_test"

def clean_data():
    if os.path.exists(DATA_DIR):
        shutil.rmtree(DATA_DIR)
    for f in ["test_users.csv", "test_mongo.json", "test_mysql.sql", "test_postgres.sql", "test_ai.csv"]:
        if os.path.exists(f):
            try:
                os.remove(f)
            except Exception:
                pass

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

def wait_for_job(job_id, timeout=10):
    start = time.time()
    while time.time() - start < timeout:
        r = requests.get(f"{BASE_URL}/migration/status/{job_id}").json()
        if r["status"] == "success" and r["result"]:
            job_info = json.loads(r["result"])
            st = job_info["status"]
            if st in ["Completed", "Failed", "Aborted"]:
                return job_info
        time.sleep(0.1)
    raise TimeoutError(f"Migration job {job_id} timed out")

def run_tests():
    proc = start_server()
    try:
        print("\n--- Test 1: Capability Matrix & Status Verification ---")
        status_res = requests.get(f"{BASE_URL}/").json()
        capabilities = status_res.get("capabilities", [])
        expected_cap = "Universal Migration Framework (Postgres, MySQL, Mongo, CSV, Schema Drift, AI Vectors)"
        assert expected_cap in capabilities, f"Missing migration capability: {capabilities}"
        print(f"✅ Status capabilities include: {expected_cap}")

        print("\n--- Test 2: Admin Session Login ---")
        login_res = requests.post(f"{BASE_URL}/login", json={
            "username": "admin",
            "password": "admin123"
        }).json()
        assert login_res["status"] == "success", f"Login failed: {login_res}"
        token = login_res["token"]
        print("✅ SuperAdmin logged in successfully.")

        print("\n--- Test 3: CSV Migration with Schema Drift (ADD_MISSING_COLUMNS) ---")
        with open("test_users.csv", "w") as f:
            f.write("id,username,email,tier,signup_year\n")
            for i in range(1, 21):
                f.write(f"{i},user_{i},user_{i}@domain.com,Gold,2026\n")

        res_mig_csv = requests.post(f"{BASE_URL}/query", json={
            "query": "MIGRATE FROM 'csv' FILE 'test_users.csv' TO TABLE 'users_migrated' SCHEMA_POLICY ADD_MISSING_COLUMNS;",
            "token": token
        }).json()
        assert res_mig_csv["status"] == "success", f"CSV migrate failed: {res_mig_csv}"
        job_id_csv = res_mig_csv["result"].split("'")[1]
        print(f"  Spawned CSV migration job: {job_id_csv}")

        info_csv = wait_for_job(job_id_csv)
        assert info_csv["status"] == "Completed"
        assert info_csv["progress"]["records_processed"] == 20
        print(f"✅ CSV migration completed! Processed: {info_csv['progress']['records_processed']} rows.")

        # Query restored data to verify schema drift added all columns
        q_res = requests.post(f"{BASE_URL}/query", json={
            "query": "SELECT * FROM users_migrated WHERE id = '5';",
            "token": token
        }).json()
        assert q_res["status"] == "success"
        rows = json.loads(q_res["result"])
        assert len(rows) == 1
        assert rows[0]["username"] == "user_5"
        assert rows[0]["tier"] == "Gold"
        assert rows[0]["signup_year"] == "2026"
        print(f"✅ Query verified drifted columns (tier='{rows[0]['tier']}', signup_year='{rows[0]['signup_year']}').")

        print("\n--- Test 4: MongoDB Extended JSON Migration ($oid, $date, $numberLong) ---")
        with open("test_mongo.json", "w") as f:
            for i in range(1, 11):
                doc = {
                    "_id": {"$oid": f"64b0f00{i}"},
                    "account_name": f"Enterprise_Client_{i}",
                    "created_at": {"$date": "2026-09-11T00:00:00Z"},
                    "annual_revenue": {"$numberLong": f"{i * 100000}"}
                }
                f.write(json.dumps(doc) + "\n")

        res_mig_mongo = requests.post(f"{BASE_URL}/query", json={
            "query": "MIGRATE FROM 'mongodb' FILE 'test_mongo.json' TO TABLE 'mongo_clients' SCHEMA_POLICY ADD_MISSING_COLUMNS;",
            "token": token
        }).json()
        assert res_mig_mongo["status"] == "success"
        job_id_mongo = res_mig_mongo["result"].split("'")[1]
        print(f"  Spawned MongoDB migration job: {job_id_mongo}")

        info_mongo = wait_for_job(job_id_mongo)
        assert info_mongo["status"] == "Completed"
        assert info_mongo["progress"]["records_processed"] == 10

        # Query normalized MongoDB document
        q_mongo = requests.post(f"{BASE_URL}/query", json={
            "query": "SELECT * FROM mongo_clients WHERE id = '64b0f003';",
            "token": token
        }).json()
        assert q_mongo["status"] == "success"
        m_rows = json.loads(q_mongo["result"])
        assert len(m_rows) == 1
        assert m_rows[0]["account_name"] == "Enterprise_Client_3"
        assert m_rows[0]["annual_revenue"] == "300000"
        print(f"✅ MongoDB Extended JSON correctly normalized: id='{m_rows[0]['id']}', annual_revenue='{m_rows[0]['annual_revenue']}'.")

        print("\n--- Test 5: MySQL Dump SQL Ingestion ---")
        with open("test_mysql.sql", "w") as f:
            f.write("-- MySQL dump 10.13\n")
            f.write("INSERT INTO `legacy_inventory` (`sku_id`, `product_name`, `price`) VALUES ")
            f.write("('SKU-100', 'Cloud Server Blade', 1299), ")
            f.write("('SKU-200', 'NVMe Storage Pod', 849), ")
            f.write("('SKU-300', 'Fiber Switch 400G', 3200);\n")

        res_mig_mysql = requests.post(f"{BASE_URL}/query", json={
            "query": "MIGRATE FROM 'mysql' FILE 'test_mysql.sql' TO TABLE 'legacy_inventory' SCHEMA_POLICY ADD_MISSING_COLUMNS;",
            "token": token
        }).json()
        assert res_mig_mysql["status"] == "success"
        job_id_mysql = res_mig_mysql["result"].split("'")[1]

        info_mysql = wait_for_job(job_id_mysql)
        assert info_mysql["status"] == "Completed"
        assert info_mysql["progress"]["records_processed"] == 3

        q_mysql = requests.post(f"{BASE_URL}/query", json={
            "query": "SELECT * FROM legacy_inventory WHERE sku_id = 'SKU-200';",
            "token": token
        }).json()
        assert q_mysql["status"] == "success"
        my_rows = json.loads(q_mysql["result"])
        assert len(my_rows) == 1
        assert my_rows[0]["product_name"] == "NVMe Storage Pod"
        print(f"✅ MySQL dump migration validated: product='{my_rows[0]['product_name']}', price='{my_rows[0]['price']}'.")

        print("\n--- Test 6: PostgreSQL Dump SQL Ingestion ---")
        with open("test_postgres.sql", "w") as f:
            f.write("-- PostgreSQL database dump\n")
            f.write("INSERT INTO pg_billing (bill_id, customer, amount) VALUES (9001, 'CorpA', 1500);\n")
            f.write("INSERT INTO pg_billing (bill_id, customer, amount) VALUES (9002, 'CorpB', 2750);\n")

        res_mig_pg = requests.post(f"{BASE_URL}/query", json={
            "query": "MIGRATE FROM 'postgres' FILE 'test_postgres.sql' TO TABLE 'pg_billing' SCHEMA_POLICY ADD_MISSING_COLUMNS;",
            "token": token
        }).json()
        assert res_mig_pg["status"] == "success"
        job_id_pg = res_mig_pg["result"].split("'")[1]

        info_pg = wait_for_job(job_id_pg)
        assert info_pg["status"] == "Completed"
        assert info_pg["progress"]["records_processed"] == 2
        print(f"✅ PostgreSQL dump migration validated: {info_pg['progress']['records_processed']} records.")

        print("\n--- Test 7: AI Auto-Embedding Pipeline (EMBED col AS vec_col DIM 384) ---")
        with open("test_ai.csv", "w") as f:
            f.write("doc_id,title,content\n")
            f.write("1,\"Coronary Care\",\"Cardiology treatments and heart catheterization procedures for myocardial infarction\"\n")
            f.write("2,\"Machine Learning\",\"Deep neural network transformer attention architectures for language understanding\"\n")

        res_mig_ai = requests.post(f"{BASE_URL}/query", json={
            "query": "MIGRATE FROM 'csv' FILE 'test_ai.csv' TO TABLE 'medical_ai' SCHEMA_POLICY ADD_MISSING_COLUMNS EMBED content AS embedding DIM 384;",
            "token": token
        }).json()
        assert res_mig_ai["status"] == "success"
        job_id_ai = res_mig_ai["result"].split("'")[1]

        info_ai = wait_for_job(job_id_ai)
        assert info_ai["status"] == "Completed"

        q_ai = requests.post(f"{BASE_URL}/query", json={
            "query": "SELECT * FROM medical_ai WHERE doc_id = '1';",
            "token": token
        }).json()
        assert q_ai["status"] == "success"
        ai_rows = json.loads(q_ai["result"])
        assert len(ai_rows) == 1
        assert "embedding" in ai_rows[0]
        assert ai_rows[0]["embedding"].startswith("[")
        assert len(ai_rows[0]["embedding"].split(",")) == 384
        print(f"✅ AI Auto-Embedding generated 384-dimensional vector for document: '{ai_rows[0]['title']}'.")

        print("\n--- Test 8: Migration Validation Engine ---")
        val_res = requests.post(f"{BASE_URL}/query", json={
            "query": f"VALIDATE MIGRATION '{job_id_csv}';",
            "token": token
        }).json()
        assert val_res["status"] == "success"
        report = json.loads(val_res["result"])
        assert report["is_valid"] == True
        assert report["row_count_match"] == True
        assert report["source_rows"] == 20
        assert report["target_rows"] == 20
        print(f"✅ Migration validation passed! Source: {report['source_rows']}, Target: {report['target_rows']}, Checksum: {report['target_checksum'][:16]}...")

        print("\n--- Test 9: REST API /migration/start, /migration/jobs, and /migration/status ---")
        api_start = requests.post(f"{BASE_URL}/migration/start", json={
            "token": token,
            "source_type": "csv",
            "source_path": "test_users.csv",
            "target_table": "users_api_migrated",
            "schema_policy": "ADD_MISSING_COLUMNS",
            "batch_size": 10
        }).json()
        assert api_start["status"] == "success"
        api_job_id = api_start["result"]

        info_api = wait_for_job(api_job_id)
        assert info_api["status"] == "Completed"

        jobs_list = requests.get(f"{BASE_URL}/migration/jobs").json()
        assert jobs_list["status"] == "success"
        all_jobs = json.loads(jobs_list["result"])
        assert len(all_jobs) >= 5
        print(f"✅ REST API verified! Successfully listed {len(all_jobs)} migration jobs.")

        print("\n🎉 ALL PHASE 13B UNIVERSAL MIGRATION TESTS PASSED PERFECTLY! 🎉\n")
    finally:
        proc.kill()
        clean_data()

if __name__ == "__main__":
    run_tests()
