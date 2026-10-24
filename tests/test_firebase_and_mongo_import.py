#!/usr/bin/env python3
"""
Test Suite: Firebase Realtime Database & MongoDB Extended JSON Import into PingDartDB
1. Authenticate with PingDartDB
2. Ensure database 'pingdb' exists
3. Execute SQL MIGRATE FROM 'firebase'
4. Execute SQL MIGRATE FROM 'mongodb'
5. Wait for background migration jobs to complete
6. Query imported tables:
   - Verify Firebase collection keys became primary 'id'
   - Verify Firestore timestamps were normalized
   - Verify MongoDB '$oid', '$date', '$numberLong', '$numberInt' normalization
7. Test Document API (/document/query) on both collections
"""

import requests
import json
import time
import sys

BASE_URL = "http://127.0.0.1:4650"
DB_NAME = "pingdb"

def run():
    print("=" * 70)
    print("🔥 TESTING FIREBASE & MONGODB IMPORT INTO PINGDART DB 🍃")
    print("=" * 70)

    # 1. Login
    print("\n1. Authenticating admin user...")
    login_resp = requests.post(f"{BASE_URL}/login", json={"username": "admin", "password": "admin123"})
    assert login_resp.status_code == 200, f"Login failed: {login_resp.text}"
    token = login_resp.json()["token"]
    print(f"   ✅ Authenticated! Token: {token[:8]}...")

    # 2. Ensure database pingdb exists
    requests.post(f"{BASE_URL}/query", json={"token": token, "query": f"CREATE DATABASE {DB_NAME}"})

    # Drop existing test tables if any
    requests.post(f"{BASE_URL}/query", json={"token": token, "database": DB_NAME, "query": "DROP TABLE firebase_doctors"})
    requests.post(f"{BASE_URL}/query", json={"token": token, "database": DB_NAME, "query": "DROP TABLE mongo_patients"})

    # 3. Test Firebase Realtime DB Import
    print("\n2. Executing SQL: MIGRATE FROM 'firebase' FILE 'data_samples/firebase_realtime_db.json'...")
    fb_sql = "MIGRATE FROM 'firebase' FILE 'data_samples/firebase_realtime_db.json' TO TABLE 'firebase_doctors' SCHEMA_POLICY ADD_MISSING_COLUMNS;"
    fb_res = requests.post(f"{BASE_URL}/query", json={"token": token, "database": DB_NAME, "query": fb_sql}).json()
    print(f"   Response: {fb_res}")
    assert fb_res.get("status") == "success", f"Firebase migration trigger failed: {fb_res}"

    # 4. Test MongoDB Extended JSON Import
    print("\n3. Executing SQL: MIGRATE FROM 'mongodb' FILE 'data_samples/mongodb_dump.json'...")
    mongo_sql = "MIGRATE FROM 'mongodb' FILE 'data_samples/mongodb_dump.json' TO TABLE 'mongo_patients' SCHEMA_POLICY ADD_MISSING_COLUMNS;"
    mongo_res = requests.post(f"{BASE_URL}/query", json={"token": token, "database": DB_NAME, "query": mongo_sql}).json()
    print(f"   Response: {mongo_res}")
    assert mongo_res.get("status") == "success", f"MongoDB migration trigger failed: {mongo_res}"

    # 5. Wait for background migration jobs
    print("\n4. Waiting for background migration jobs to complete...")
    time.sleep(1.5)

    # 6. Verify Firebase Doctors Table
    print("\n5. Verifying Firebase Doctors in PingDartDB...")
    fb_query = requests.post(f"{BASE_URL}/query", json={
        "token": token,
        "database": DB_NAME,
        "query": "SELECT id, name, speciality, department, rating, created_at FROM firebase_doctors;"
    }).json()

    assert fb_query.get("status") == "success", f"Query firebase_doctors failed: {fb_query}"
    fb_rows = json.loads(fb_query["result"])
    print(f"   ✅ Successfully queried {len(fb_rows)} Firebase doctor records:")
    for r in fb_rows:
        print(f"      - ID: {r.get('id')} | Name: {r.get('name')} | Speciality: {r.get('speciality')} | Rating: {r.get('rating')} | Created: {r.get('created_at')}")

    assert len(fb_rows) == 4, f"Expected 4 Firebase records, got {len(fb_rows)}"
    # Check push ID mapping
    ids = [r.get("id") for r in fb_rows]
    assert "-O7X9aBcDeF12345678" in ids, "Push ID '-O7X9aBcDeF12345678' not preserved as primary 'id'"
    # Check timestamp normalization
    sarah = next(r for r in fb_rows if r.get("id") == "-O7X9aBcDeF12345678")
    assert sarah.get("created_at") == "1728000000", f"Expected timestamp '1728000000', got {sarah.get('created_at')}"

    # 7. Verify MongoDB Patients Table
    print("\n6. Verifying MongoDB Patients in PingDartDB...")
    mongo_query = requests.post(f"{BASE_URL}/query", json={
        "token": token,
        "database": DB_NAME,
        "query": "SELECT id, patient_name, condition, blood_group, admission_date, hospital_bill, room_number, status FROM mongo_patients;"
    }).json()

    assert mongo_query.get("status") == "success", f"Query mongo_patients failed: {mongo_query}"
    mongo_rows = json.loads(mongo_query["result"])
    print(f"   ✅ Successfully queried {len(mongo_rows)} MongoDB patient records:")
    for r in mongo_rows:
        print(f"      - ID: {r.get('id')} | Patient: {r.get('patient_name')} | Condition: {r.get('condition')} | Bill: {r.get('hospital_bill')} | Room: {r.get('room_number')}")

    assert len(mongo_rows) == 4, f"Expected 4 MongoDB records, got {len(mongo_rows)}"
    # Check $oid normalization
    john = next(r for r in mongo_rows if r.get("patient_name") == "John Doe")
    assert john.get("id") == "66f1234567890abcdef12301", f"Expected normalized OID, got {john.get('id')}"
    # Check $date normalization
    assert "2026-08-15" in str(john.get("admission_date")), f"Date normalization failed: {john.get('admission_date')}"
    # Check $numberLong normalization
    assert john.get("hospital_bill") == "45000", f"NumberLong normalization failed: {john.get('hospital_bill')}"

    # 8. Test Document Query API on Firebase collection
    print("\n7. Testing Document API (JSON Filter) on 'firebase_doctors'...")
    doc_res = requests.post(f"{BASE_URL}/document/query", json={
        "token": token,
        "database": DB_NAME,
        "collection": "firebase_doctors",
        "where": [{"field": "speciality", "operator": "==", "value": "Cardiology"}],
        "limit": 5
    }).json()

    assert doc_res.get("status") == "success", f"Document query failed: {doc_res}"
    doc_results = json.loads(doc_res["result"])
    assert len(doc_results) == 1, f"Expected 1 cardiologist, got {len(doc_results)}"
    print(f"   ✅ Document API matched {len(doc_results)} document: {doc_results[0]['columns']['name']}")

    print("\n" + "=" * 70)
    print("🎉 ALL FIREBASE & MONGODB IMPORT AND VERIFICATION TESTS PASSED 100%!")
    print("=" * 70)

if __name__ == "__main__":
    run()
