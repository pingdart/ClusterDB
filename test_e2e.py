import requests
import json
import time
import sys
import os

BASE_URL = "http://localhost:4650"
DB_NAME = "test_db"

def test_login():
    print("--- Testing Login ---")
    try:
        resp = requests.post(f"{BASE_URL}/login", json={
            "username": "admin",
            "password": "admin123"
        })
        resp.raise_for_status()
        data = resp.json()
        assert data.get("status") == "success", f"Login failed: {data}"
        token = data.get("token")
        print(f"✅ Login successful. Token: {token[:8]}...")
        return token
    except requests.exceptions.ConnectionError:
        print(f"❌ Connection error: Make sure PingDartDB is running on {BASE_URL}")
        sys.exit(1)

def test_sql_api(token):
    print("\n--- Testing SQL API (DDL & Insert) ---")
    
    # 1. Create Database
    resp = requests.post(f"{BASE_URL}/query", json={
        "token": token,
        "query": f"CREATE DATABASE {DB_NAME}"
    })
    print(f"Create DB ({DB_NAME}):", resp.json().get("result") or resp.json().get("error"))
    
    # 2. Create Table
    resp = requests.post(f"{BASE_URL}/query", json={
        "token": token,
        "database": DB_NAME,
        "query": "CREATE TABLE users (id INT AUTO_INCREMENT PRIMARY KEY, name STRING, city STRING)"
    })
    print("Create Table 'users':", resp.json().get("result") or resp.json().get("error"))

    # 3. Insert Data
    for name, city in [("Alice", "Hyderabad"), ("Bob", "Bangalore"), ("Charlie", "Hyderabad")]:
        resp = requests.post(f"{BASE_URL}/query", json={
            "token": token,
            "database": DB_NAME,
            "query": f"INSERT INTO users (name, city) VALUES ('{name}', '{city}')"
        })
        print(f"Insert {name}:", resp.json().get("result") or resp.json().get("error"))

def test_document_api(token):
    print("\n--- Testing Document API (JSON Querying) ---")
    
    # Query for users in Hyderabad
    payload = {
        "token": token,
        "database": DB_NAME,
        "collection": "users",
        "where": [
            {
                "field": "city",
                "operator": "==",
                "value": "Hyderabad"
            }
        ],
        "limit": 10
    }
    
    print(f"Sending Document Query Payload:\n{json.dumps(payload, indent=2)}")
    
    resp = requests.post(f"{BASE_URL}/document/query", json=payload)
    data = resp.json()
    
    if data.get("status") == "success":
        print("✅ Document Query Successful!")
        # The result string contains the JSON-encoded documents
        results = json.loads(data["result"])
        print(f"\nFound {len(results)} matching documents in '{DB_NAME}_users':")
        for doc in results:
            print(" ->", json.dumps(doc, indent=2))
            
        # Verify the results are filtered correctly
        assert len(results) > 0, "Expected at least 1 document"
        for doc in results:
            assert doc["columns"]["city"] == "Hyderabad" or doc.get("city") == "Hyderabad", "Filter failed!"
    else:
        print("❌ Document Query Failed:", data)
        raise AssertionError("Document API failed")

def test_join_api(token):
    print("\n--- Testing SQL API (JOIN) ---")
    requests.post(f"{BASE_URL}/query", json={
        "token": token,
        "database": DB_NAME,
        "query": "CREATE TABLE profiles (id INT AUTO_INCREMENT PRIMARY KEY, user_id STRING, bio STRING)"
    })
    
    for user_id, bio in [("1", "Alice is from HYD"), ("2", "Bob is a techie")]:
        requests.post(f"{BASE_URL}/query", json={
            "token": token,
            "database": DB_NAME,
            "query": f"INSERT INTO profiles (user_id, bio) VALUES ('{user_id}', '{bio}')"
        })
        
    resp = requests.post(f"{BASE_URL}/query", json={
        "token": token,
        "database": DB_NAME,
        "query": "SELECT * FROM profiles JOIN users ON users.id = profiles.user_id WHERE profiles.id = '1'"
    })
    data = resp.json()
    if data.get("status") == "success":
        print("✅ JOIN Query Successful!")
        results = json.loads(data["result"])
        print(f"Found {len(results)} joined rows:")
        for r in results:
            print(" ->", json.dumps(r, indent=2))
        assert len(results) == 1, "Expected 1 joined row due to point lookup"
    else:
        print("❌ JOIN Query Failed:", data)
        raise AssertionError("JOIN API failed")

def test_bulk_insert(token):
    print("\n--- Testing Bulk Insert (150 rows auto text) ---")
    requests.post(f"{BASE_URL}/query", json={
        "token": token,
        "database": DB_NAME,
        "query": "CREATE TABLE logs (id INT AUTO_INCREMENT PRIMARY KEY, message STRING)"
    })
    
    print("Inserting 150 rows...")
    success_count = 0
    for i in range(150):
        resp = requests.post(f"{BASE_URL}/query", json={
            "token": token,
            "database": DB_NAME,
            "query": f"INSERT INTO logs (message) VALUES ('Auto text message {i}')"
        })
        if resp.json().get("status") == "success":
            success_count += 1
            
    print(f"✅ Inserted {success_count}/150 rows successfully.")
    assert success_count == 150, "Bulk insert failed"

def test_geo_search(token):
    print("\n--- Testing Geo-Spatial Search ---")
    requests.post(f"{BASE_URL}/query", json={
        "token": token,
        "database": DB_NAME,
        "query": "CREATE TABLE places (id INT AUTO_INCREMENT PRIMARY KEY, name STRING, lat STRING, lon STRING)"
    })
    
    for name, lat, lon in [("Near Place", "37.7849", "-122.4094"), ("Far Place", "37.6000", "-122.2000")]:
        requests.post(f"{BASE_URL}/query", json={
            "token": token,
            "database": DB_NAME,
            "query": f"INSERT INTO places (name, lat, lon) VALUES ('{name}', '{lat}', '{lon}')"
        })
        
    payload = {
        "token": token,
        "database": DB_NAME,
        "collection": "places",
        "near": {
            "lat": 37.7749,
            "lon": -122.4194,
            "radius_km": 5.0
        }
    }
    resp = requests.post(f"{BASE_URL}/document/query", json=payload)
    data = resp.json()
    
    if data.get("status") == "success":
        results = json.loads(data["result"])
        print(f"Found {len(results)} places within 5km:")
        for r in results:
            print(" ->", json.dumps(r, indent=2))
        assert len(results) == 1, "Expected exactly 1 near place"
        assert results[0]["columns"]["name"] == "Near Place", "Expected 'Near Place'"
        assert "_distance_km" in results[0], "Expected distance to be calculated"
        print("✅ Geo-Spatial Search Successful!")
    else:
        print("❌ Geo-Spatial Search Failed:", data)
        raise AssertionError("Geo-Spatial Search API failed")

def test_database_export(token):
    print("\n--- Testing Database Export API ---")
    resp = requests.post(f"{BASE_URL}/database/export", json={
        "token": token,
        "database": DB_NAME
    })
    data = resp.json()
    if data.get("status") == "success":
        export_data = data.get("data", {})
        print(f"✅ Export Successful! Found {len(export_data)} collections.")
        print(f"Collections exported: {list(export_data.keys())}")
        
        assert "users" in export_data, "Missing users collection in export"
        assert len(export_data["users"]) >= 3, "Expected at least 3 users exported"
        
        with open("export_dump.json", "w") as f:
            json.dump(export_data, f, indent=2)
        print("💾 Dump saved to export_dump.json")
    else:
        print("❌ Database Export Failed:", data)
        raise AssertionError("Export API failed")

def test_database_import(token):
    print("\n--- Testing Database Import API ---")
    if not os.path.exists("export_dump.json"):
        print("⏭️ Skipping import test (export_dump.json not found)")
        return
        
    with open("export_dump.json", "r") as f:
        export_data = json.load(f)
        
    resp = requests.post(f"{BASE_URL}/database/import", json={
        "token": token,
        "database": "imported_db",
        "data": export_data
    })
    data = resp.json()
    if data.get("status") == "success":
        print(f"✅ Import Successful! Restored {data.get('imported_collections')} collections and {data.get('imported_rows')} rows.")
    else:
        print("❌ Database Import Failed:", data)
        raise AssertionError("Import API failed")

def test_multi_statement(token):
    print("\n--- Testing Multi-Statement SQL Execution ---")
    query = """
    CREATE TABLE t1 (id INT AUTO_INCREMENT PRIMARY KEY, val STRING);
    CREATE TABLE t2 (id INT AUTO_INCREMENT PRIMARY KEY, val STRING);
    INSERT INTO t1 (val) VALUES ('hello');
    INSERT INTO t2 (val) VALUES ('world');
    """
    resp = requests.post(f"{BASE_URL}/query", json={
        "token": token,
        "database": DB_NAME,
        "query": query
    })
    data = resp.json()
    if data.get("status") == "success":
        print("✅ Multi-Statement Query Successful!")
    else:
        print("❌ Multi-Statement Query Failed:", data)
        raise AssertionError("Multi-Statement API failed")

if __name__ == "__main__":
    print("=======================================")
    print("PingDartDB - Automated Integration Test")
    print("=======================================")
    
    token = test_login()
    time.sleep(0.5) # Slight delay for safety
    
    try:
        test_multi_statement(token)
        test_sql_api(token)
        test_document_api(token)
        test_join_api(token)
        test_bulk_insert(token)
        test_geo_search(token)
        test_database_export(token)
        test_database_import(token)
        print("\n🎉 All automated tests completed successfully!")
    except AssertionError as e:
        print("\n❌ Test Assertion Failed:", e)
    except Exception as e:
        print("\n❌ Unexpected Error:", e)
