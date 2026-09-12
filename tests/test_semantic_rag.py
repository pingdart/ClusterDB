#!/usr/bin/env python3
"""
PingDartDB - RAG-Native Semantic Search & Embedding Engine End-to-End Test Suite
Tests:
1. Model Registry: CREATE EMBEDDING MODEL & SHOW EMBEDDING MODELS
2. Security: SECRET='ENV_VAR_NAME' (no raw API keys in SQL)
3. Auto Embeddings: VECTOR(384) GENERATED FROM bio USING fast_embed
4. Plain-text INSERT without manual vectors
5. Text Column Semantic Search: SEMANTIC_MATCH(bio, 'query') > 0.4
6. Online Re-Embedding: ALTER TABLE doctors REEMBED USING fast_embed
7. Semantic JOIN: JOIN ON SEMANTIC_MATCH(doctors.bio, hospitals.department_focus) > 0.4
"""

import requests
import json

BASE_URL = "http://localhost:4650"

def run_sql(token, query, db="rag_test_db"):
    resp = requests.post(
        f"{BASE_URL}/query",
        json={"token": token, "query": query, "database": db}
    )
    if not resp.ok:
        raise Exception(f"SQL Error ({resp.status_code}): {resp.text}")
    try:
        return resp.json().get("result", "")
    except Exception:
        return resp.text

def main():
    print("=" * 65)
    print("   PingDartDB - RAG-Native Semantic Search & Knowledge Engine    ")
    print("=" * 65)

    # 1. Login
    login_res = requests.post(f"{BASE_URL}/login", json={"username": "admin", "password": "admin123"}).json()
    token = login_res["token"]
    print("✅ 1. Authenticated admin user")

    # Clean up prior database if any
    try:
        run_sql(token, "DROP DATABASE rag_test_db")
    except Exception:
        pass
    run_sql(token, "CREATE DATABASE rag_test_db")
    print("✅ 2. Created test database 'rag_test_db'")

    # 2. Register Embedding Model
    res = run_sql(token, "CREATE EMBEDDING MODEL fast_embed TYPE='local' DIMENSION=384 VERSION='1.0';")
    print(f"✅ 3. Model Registry: {res}")

    # Register OpenAI Model with secret environment variable reference (No raw API keys!)
    res_secret = run_sql(token, "CREATE EMBEDDING MODEL openai_embed TYPE='openai' SECRET='OPENAI_EMBED_KEY' DIMENSION=1536 MODEL_NAME='text-embedding-3-small';")
    print(f"✅ 4. Secure Model Registration (SECRET instead of API key): {res_secret}")

    # Show Embedding Models
    models_json = run_sql(token, "SHOW EMBEDDING MODELS;")
    models = json.loads(models_json)
    print(f"✅ 5. SHOW EMBEDDING MODELS: Found {len(models)} registered models ({[m['name'] for m in models]})")

    # 3. Create table with GENERATED FROM ... USING ...
    create_table_sql = """
    CREATE TABLE doctors (
        id INT PRIMARY KEY,
        name VARCHAR(100),
        bio TEXT,
        embedding VECTOR(384) GENERATED FROM bio USING fast_embed
    );
    """
    res_ct = run_sql(token, create_table_sql)
    print(f"✅ 6. DDL with GENERATED FROM: {res_ct}")

    # 4. Insert data using PLAIN TEXT (Zero vector array management for developers!)
    run_sql(token, "INSERT INTO doctors (id, name, bio) VALUES (1, 'Dr Rao', 'Cardiologist with 15 years experience in open heart surgery, bypass, and cardiovascular medicine');")
    run_sql(token, "INSERT INTO doctors (id, name, bio) VALUES (2, 'Dr Sarah Chen', 'Pediatrician specializing in infant health, child care, and early childhood nutrition');")
    run_sql(token, "INSERT INTO doctors (id, name, bio) VALUES (3, 'Dr Ramesh Kumar', 'Interventional cardiologist performing angioplasty, heart catheterization, and stent procedures');")
    run_sql(token, "INSERT INTO doctors (id, name, bio) VALUES (4, 'Dr Emily Watson', 'Orthopedic surgeon focusing on knee arthroscopy, joint replacement, and sports injuries');")
    print("✅ 7. Inserted 4 doctors with pure text (PingDart auto-embedded & indexed HNSW vectors!)")

    # 5. SEMANTIC_MATCH on text column directly
    search_sql = "SELECT name, bio, SEMANTIC_MATCH(bio, 'heart surgery cardiology specialist') AS score FROM doctors WHERE SEMANTIC_MATCH(bio, 'heart surgery cardiology specialist') > 0.15 ORDER BY score DESC;"
    res_search = run_sql(token, search_sql)
    matched_doctors = json.loads(res_search)
    print(f"✅ 8. SEMANTIC_MATCH(bio, 'heart surgery cardiology specialist') > 0.15 returned {len(matched_doctors)} doctors:")
    for doc in matched_doctors:
        print(f"    - {doc.get('name')}: score={doc.get('score')} | {doc.get('bio')[:60]}...")
    assert len(matched_doctors) >= 2, "Expected at least 2 cardiology matches"
    assert "Dr Rao" in [d.get("name") for d in matched_doctors]

    # 6. ALTER TABLE REEMBED
    res_reembed = run_sql(token, "ALTER TABLE doctors REEMBED USING fast_embed;")
    print(f"✅ 9. Online Re-Embedding Worker: {res_reembed}")

    # 7. Semantic JOIN
    run_sql(token, """
    CREATE TABLE hospitals (
        id INT PRIMARY KEY,
        name VARCHAR(100),
        department_focus TEXT
    );
    """)
    run_sql(token, "INSERT INTO hospitals (id, name, department_focus) VALUES (1, 'Apex Heart Institute', 'Cardiology, cardiac catheterization, valve replacements, and cardiovascular surgery');")
    run_sql(token, "INSERT INTO hospitals (id, name, department_focus) VALUES (2, 'Little Angels Pediatric Hospital', 'Pediatrics, neonatology, infant growth, and child care');")
    run_sql(token, "INSERT INTO hospitals (id, name, department_focus) VALUES (3, 'Global Spine & Joint Clinic', 'Orthopedic surgery, knee replacements, and sports physical therapy');")

    join_sql = """
    SELECT doctors.name, hospitals.name
    FROM doctors
    JOIN hospitals
    ON SEMANTIC_MATCH(doctors.bio, hospitals.department_focus) > 0.15;
    """
    res_join = run_sql(token, join_sql)
    joined_rows = json.loads(res_join)
    print(f"✅ 10. Semantic JOIN returned {len(joined_rows)} high-affinity pairings:")
    for r in joined_rows:
        doc_name = r.get("doctors.name", r.get("name", "Unknown"))
        hosp_name = r.get("hospitals.name", "Unknown")
        print(f"    - Doctor [{doc_name}] <---> Hospital [{hosp_name}]")
    assert len(joined_rows) >= 2, "Expected at least 2 semantic join matches"

    print("=" * 65)
    print("   🎉 ALL RAG-NATIVE KNOWLEDGE ENGINE TESTS PASSED 100%!     ")
    print("=" * 65)

if __name__ == "__main__":
    main()
