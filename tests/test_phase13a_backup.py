import os
import sys
import time
import json
import shutil
import subprocess
import requests

SERVER_PORT = 4799
BASE_URL = f"http://127.0.0.1:{SERVER_PORT}"
DATA_DIR = "data_phase13a_test"
MAGIC_BYTES = b"PINGDART_PB\x01"

def clean_data():
    if os.path.exists(DATA_DIR):
        shutil.rmtree(DATA_DIR)
    for f in ["test_backup.pb", "test_encrypted.pb", "test_tampered.pb", "test_cluster_api.pb"]:
        if os.path.exists(f):
            os.remove(f)

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
        assert "Enterprise Native Backup Engine (.pb Protobuf, ZSTD, Encryption)" in capabilities, \
            f"Missing backup capability: {capabilities}"
        print("✅ Status capabilities include Enterprise Native Backup Engine (.pb Protobuf, ZSTD, Encryption).")

        print("\n--- Test 2: Admin Session Login ---")
        login_res = requests.post(f"{BASE_URL}/login", json={
            "username": "admin",
            "password": "admin123"
        }).json()
        assert login_res["status"] == "success", f"Login failed: {login_res}"
        token = login_res["token"]
        print("✅ SuperAdmin logged in successfully.")

        print("\n--- Test 3: Seed Data & Generate Statistics ---")
        # Create tables and insert rows
        res1 = requests.post(f"{BASE_URL}/query", json={
            "query": "CREATE TABLE customers (id INT PRIMARY KEY, name TEXT, tier TEXT, spend INT);",
            "token": token
        }).json()
        assert res1["status"] == "success", f"Failed to create table: {res1}"

        for i in range(1, 26):
            tier = "Enterprise" if i % 2 == 0 else "Pro"
            requests.post(f"{BASE_URL}/query", json={
                "query": f"INSERT INTO customers (id, name, tier, spend) VALUES ({i}, 'Customer_{i}', '{tier}', {i * 100});",
                "token": token
            })

        # Run ANALYZE TABLE to create CBO stats
        res_analyze = requests.post(f"{BASE_URL}/query", json={
            "query": "ANALYZE TABLE customers;",
            "token": token
        }).json()
        assert res_analyze["status"] == "success"
        print("✅ Seeded 25 customer rows and collected persistent table statistics.")

        print("\n--- Test 4: SQL Native Protobuf + ZSTD Export ---")
        export_sql = "EXPORT DATABASE default TO 'test_backup.pb';"
        res_exp = requests.post(f"{BASE_URL}/query", json={
            "query": export_sql,
            "token": token
        }).json()
        assert res_exp["status"] == "success", f"Export failed: {res_exp}"
        exp_report = json.loads(res_exp["result"])
        assert exp_report["rows_count"] >= 25
        assert exp_report["tables_count"] >= 1
        assert exp_report["is_encrypted"] == False
        assert exp_report["compression_ratio"] > 0
        assert os.path.exists("test_backup.pb"), "File test_backup.pb not created"
        
        # Verify binary magic bytes and flags
        with open("test_backup.pb", "rb") as bf:
            header = bf.read(13)
            assert header[:12] == MAGIC_BYTES, f"Invalid magic bytes: {header[:12]}"
            flags = header[12]
            assert flags & 0x01 != 0, "ZSTD flag missing in container"
        print(f"✅ SQL EXPORT completed! Total Rows: {exp_report['rows_count']}, Uncompressed: {exp_report['uncompressed_bytes']}B -> Compressed: {exp_report['compressed_bytes']}B (Saved {exp_report['compression_ratio']:.1f}%).")

        print("\n--- Test 5: Drop Table and SQL Native Protobuf Import Restoration ---")
        requests.post(f"{BASE_URL}/query", json={
            "query": "DROP TABLE customers;",
            "token": token
        })
        # Check table is gone
        res_check_gone = requests.post(f"{BASE_URL}/query", json={
            "query": "SELECT * FROM customers;",
            "token": token
        }).json()
        assert res_check_gone["status"] == "error", "Table customers should be dropped"

        # Import from .pb
        import_sql = "IMPORT DATABASE FROM 'test_backup.pb';"
        res_imp = requests.post(f"{BASE_URL}/query", json={
            "query": import_sql,
            "token": token
        }).json()
        assert res_imp["status"] == "success", f"Import failed: {res_imp}"
        imp_report = json.loads(res_imp["result"])
        assert imp_report["tables_restored"] >= 1
        assert imp_report["rows_restored"] >= 25
        assert imp_report["checksum_verified"] == True

        # Verify data query after restore
        res_restored = requests.post(f"{BASE_URL}/query", json={
            "query": "SELECT * FROM customers WHERE id = 10;",
            "token": token
        }).json()
        assert res_restored["status"] == "success"
        rows = json.loads(res_restored["result"])
        assert len(rows) == 1
        assert rows[0]["name"] == "Customer_10"
        print(f"✅ SQL IMPORT completed! Restored {imp_report['tables_restored']} tables, {imp_report['rows_restored']} rows. Verification query succeeded: {rows[0]['name']}")

        print("\n--- Test 6: AES-256-GCM Encrypted Export and Key Protection ---")
        enc_sql = "EXPORT DATABASE default TO 'test_encrypted.pb' ENCRYPTED KEY 'SecretPassphrase123!';"
        res_enc_exp = requests.post(f"{BASE_URL}/query", json={
            "query": enc_sql,
            "token": token
        }).json()
        assert res_enc_exp["status"] == "success", f"Encrypted export failed: {res_enc_exp}"
        enc_report = json.loads(res_enc_exp["result"])
        assert enc_report["is_encrypted"] == True

        # Inspect binary flags
        with open("test_encrypted.pb", "rb") as ef:
            header = ef.read(13)
            flags = header[12]
            assert flags & 0x02 != 0, "AES-256 encryption flag missing"

        # Attempt restore without key -> MUST FAIL
        res_no_key = requests.post(f"{BASE_URL}/query", json={
            "query": "IMPORT DATABASE FROM 'test_encrypted.pb';",
            "token": token
        }).json()
        assert res_no_key["status"] == "error"
        assert "encrypted" in res_no_key["error"].lower() or "decryption" in res_no_key["error"].lower()
        print("✅ Correctly rejected import without decryption key.")

        # Attempt restore with wrong key -> MUST FAIL
        res_wrong_key = requests.post(f"{BASE_URL}/query", json={
            "query": "IMPORT DATABASE FROM 'test_encrypted.pb' KEY 'WrongKey123';",
            "token": token
        }).json()
        assert res_wrong_key["status"] == "error"
        print("✅ Correctly rejected import with invalid decryption key.")

        # Restore with valid key -> MUST SUCCEED
        res_valid_key = requests.post(f"{BASE_URL}/query", json={
            "query": "IMPORT DATABASE FROM 'test_encrypted.pb' KEY 'SecretPassphrase123!';",
            "token": token
        }).json()
        assert res_valid_key["status"] == "success"
        valid_imp_report = json.loads(res_valid_key["result"])
        assert valid_imp_report["checksum_verified"] == True
        print("✅ Encrypted import with valid AES-256 key succeeded and verified checksum!")

        print("\n--- Test 7: Tamper Protection & SHA-256 Checksum Rejection ---")
        with open("test_backup.pb", "rb") as sf:
            raw_content = bytearray(sf.read())
        # Flip bit in payload area (after 12 magic + 1 flag + 32 checksum = 45 bytes)
        raw_content[50] ^= 0xFF
        with open("test_tampered.pb", "wb") as tf:
            tf.write(raw_content)

        res_tampered = requests.post(f"{BASE_URL}/query", json={
            "query": "IMPORT DATABASE FROM 'test_tampered.pb';",
            "token": token
        }).json()
        assert res_tampered["status"] == "error"
        assert "checksum" in res_tampered["error"].lower() or "integrity" in res_tampered["error"].lower()
        print(f"✅ Tampered snapshot correctly rejected with error: {res_tampered['error']}")

        print("\n--- Test 8: REST API /backup/export and /backup/import ---")
        api_exp_res = requests.post(f"{BASE_URL}/backup/export", json={
            "token": token,
            "export_type": "cluster",
            "file_path": "test_cluster_api.pb",
            "encrypt": False
        }).json()
        assert api_exp_res["status"] == "success", f"REST export failed: {api_exp_res}"
        api_exp_report = json.loads(api_exp_res["result"])
        assert api_exp_report["scope"] == "cluster"
        assert os.path.exists("test_cluster_api.pb")

        api_imp_res = requests.post(f"{BASE_URL}/backup/import", json={
            "token": token,
            "file_path": "test_cluster_api.pb"
        }).json()
        assert api_imp_res["status"] == "success", f"REST import failed: {api_imp_res}"
        api_imp_report = json.loads(api_imp_res["result"])
        assert api_imp_report["checksum_verified"] == True
        print(f"✅ REST API /backup/export and /backup/import succeeded! Tables: {api_imp_report['tables_restored']}, Rows: {api_imp_report['rows_restored']}")

        print("\n🎉 ALL PHASE 13A TESTS PASSED SUCCESSFULLY! 🎉\n")
    finally:
        proc.kill()
        clean_data()

if __name__ == "__main__":
    run_tests()
