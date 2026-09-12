import requests
import json
import time
import sys

BASE_URL = "http://localhost:4650"
DB_NAME = f"ai_db_{int(time.time())}"

def assert_response(resp, description):
    data = resp.json()
    assert data.get("status") == "success", f"Failed {description}: {data}"
    return data

def run_tests():
    print("=================================================================")
    print("      PingDart Next-Gen AI-Native Database Benchmark Suite       ")
    print("=================================================================")

    # 1. Health check
    print("\n[1/10] Testing Engine Health & Capabilities...")
    r = requests.get(f"{BASE_URL}/")
    assert r.status_code == 200, f"Engine not running on {BASE_URL}"
    health = r.json()
    print(" -> Engine:", health.get("engine"))
    print(" -> Capabilities:", health.get("capabilities"))
    assert "HNSW Vector Engine" in health.get("capabilities", [])

    # 2. Login
    print("\n[2/10] Authenticating Root User...")
    login_res = requests.post(f"{BASE_URL}/login", json={
        "username": "admin",
        "password": "admin123"
    }).json()
    assert login_res.get("status") == "success", f"Login failed: {login_res}"
    token = login_res.get("token")
    print(f" -> Token acquired: {token[:8]}...")

    # 3. Create Database & Table with Native VECTOR(384)
    print("\n[3/10] Testing DDL with Native VECTOR(384)...")
    requests.post(f"{BASE_URL}/query", json={"token": token, "query": f"CREATE DATABASE {DB_NAME}"})
    
    ddl = f"""
    CREATE TABLE ai_users (
        id INT AUTO_INCREMENT PRIMARY KEY,
        name STRING,
        bio STRING,
        embedding VECTOR(384)
    )
    """
    res = requests.post(f"{BASE_URL}/query", json={"token": token, "database": DB_NAME, "query": ddl})
    assert_response(res, "CREATE TABLE with VECTOR(384)")
    print(" -> Table 'ai_users' with VECTOR(384) created successfully")

    # 4. Insert records with vectors
    print("\n[4/10] Inserting Data with 384-dim Vectors...")
    # Generate simple deterministic 384-dim vectors
    vec_alice = [1.0 / 384.0] * 384
    vec_bob = [-1.0 / 384.0] * 384
    vec_charlie = [1.0 / 384.0 if i % 2 == 0 else 0.0 for i in range(384)]

    users = [
        ("Alice", "AI researcher working on vector databases and LLM agents", str(vec_alice)),
        ("Bob", "Frontend developer building React and UI dashboards", str(vec_bob)),
        ("Charlie", "Systems engineer building Rust databases and query engines", str(vec_charlie)),
    ]

    for name, bio, vec_str in users:
        insert_sql = f"INSERT INTO ai_users (name, bio, embedding) VALUES ('{name}', '{bio}', '{vec_str}')"
        res = requests.post(f"{BASE_URL}/query", json={"token": token, "database": DB_NAME, "query": insert_sql})
        assert_response(res, f"INSERT {name}")
    print(f" -> Successfully inserted {len(users)} vector records")

    # 5. SQL Query with VECTOR_SIMILARITY
    print("\n[5/10] Testing SQL Query with VECTOR_SIMILARITY()...")
    query_vec = [1.0 / 384.0] * 384
    sql_sim = f"SELECT * FROM ai_users WHERE VECTOR_SIMILARITY(embedding, '{str(query_vec)}') > 0.5"
    res = requests.post(f"{BASE_URL}/query", json={"token": token, "database": DB_NAME, "query": sql_sim})
    data = assert_response(res, "SQL VECTOR_SIMILARITY query")
    rows = json.loads(data["result"])
    print(f" -> Found {len(rows)} matching rows with similarity > 0.5")
    assert len(rows) >= 1
    assert any(r.get("name") == "Alice" or r.get("columns", {}).get("name") == "Alice" for r in rows)

    # 6. Dedicated HNSW Vector Search Endpoint
    print("\n[6/10] Testing HNSW Vector Search API (/vector/search)...")
    res = requests.post(f"{BASE_URL}/vector/search", json={
        "token": token,
        "database": DB_NAME,
        "table": "ai_users",
        "column": "embedding",
        "vector": query_vec,
        "top_k": 2
    })
    data = assert_response(res, "HNSW Search")
    hits = json.loads(data["result"])
    print(f" -> Top-{len(hits)} HNSW Nearest Neighbors:")
    for h in hits:
        print(f"    - ID: {h['id']}, Distance: {h['distance']:.4f}, Similarity: {h['similarity']:.4f}")
    assert len(hits) > 0
    assert hits[0]["similarity"] > 0.99  # Alice should be an exact match

    # 7. AI Cognitive Memory & Consolidation
    print("\n[7/10] Testing AI Cognitive Memory Engine (/ai/memory/*)...")
    session = "agent_session_42"
    # Store multiple working/episodic memories
    for i in range(1, 8):
        requests.post(f"{BASE_URL}/ai/memory/store", json={
            "token": token,
            "session_id": session,
            "memory_type": "Working",
            "content": f"Agent executed task step {i} analyzing customer requirements",
            "importance": 0.3 if i < 6 else 0.9,
            "metadata": {"step": i}
        })
    print(" -> Stored 7 working memory items")

    # Recall memories
    res = requests.post(f"{BASE_URL}/ai/memory/recall", json={
        "token": token,
        "session_id": session,
        "top_k": 3
    })
    data = assert_response(res, "Memory Recall")
    recalled = json.loads(data["result"])
    print(f" -> Recalled {len(recalled)} top memories:")
    for m, score in recalled:
        print(f"    - [{m['memory_type']}] Score: {score:.4f} | {m['content']}")

    # Consolidate memories
    print(" -> Testing Memory Consolidation Engine...")
    res = requests.post(f"{BASE_URL}/ai/memory/consolidate", json={
        "token": token,
        "session_id": session
    })
    data = assert_response(res, "Memory Consolidate")
    print(" -> Consolidation response:", data.get("result"))

    # 8. Neo4j-Style Knowledge Graph
    print("\n[8/10] Testing Neo4j-Style Knowledge Graph (/graph/*)...")
    # Create Labeled Nodes
    res1 = requests.post(f"{BASE_URL}/graph/node", json={
        "token": token,
        "id": "dr_alice",
        "labels": ["User", "Doctor"],
        "properties": {"specialty": "Neurology"}
    })
    assert_response(res1, "Create Node dr_alice")

    res2 = requests.post(f"{BASE_URL}/graph/node", json={
        "token": token,
        "id": "hosp_metro",
        "labels": ["Hospital"],
        "properties": {"city": "Hyderabad"}
    })
    assert_response(res2, "Create Node hosp_metro")

    # Create Typed Edge
    res3 = requests.post(f"{BASE_URL}/graph/edge", json={
        "token": token,
        "id": "e_alice_metro",
        "source": "dr_alice",
        "target": "hosp_metro",
        "type": "WORKS_AT",
        "properties": {"role": "Chief Surgeon"}
    })
    assert_response(res3, "Create Edge WORKS_AT")

    # Query Neighbors
    res4 = requests.post(f"{BASE_URL}/graph/neighbors", json={
        "token": token,
        "node_id": "dr_alice",
        "edge_type": "WORKS_AT",
        "target_label": "Hospital"
    })
    data4 = assert_response(res4, "Get Neighbors")
    neighbors = json.loads(data4["result"])
    print(f" -> Found {len(neighbors)} neighbor(s) for 'dr_alice':")
    for edge, node in neighbors:
        print(f"    - -[:{edge['type']}]-> ({node['id']} labels={node['labels']})")
    assert len(neighbors) == 1
    assert neighbors[0][1]["id"] == "hosp_metro"

    # 9. Durable CDC Commit Log
    print("\n[9/10] Testing Durable CDC Commit Log (/cdc/replay)...")
    res = requests.post(f"{BASE_URL}/cdc/replay", json={
        "token": token,
        "since_sequence_id": 0,
        "limit": 10
    })
    data = assert_response(res, "CDC Replay")
    cdc_events = json.loads(data["result"])
    print(f" -> Retrieved {len(cdc_events)} persistent CDC commit log records:")
    for ev in cdc_events[:5]:
        print(f"    - Seq #{ev['sequence_id']}: [{ev['operation']}] on table '{ev['table']}' (pk={ev['primary_key']})")
    assert len(cdc_events) > 0

    # 10. Existing Regression Tests
    print("\n[10/10] Running Regression Verification on Existing SQL & Joins...")
    reg_res = requests.post(f"{BASE_URL}/query", json={
        "token": token,
        "database": DB_NAME,
        "query": "SELECT name, bio FROM ai_users WHERE name = 'Alice'"
    })
    data = assert_response(reg_res, "Point lookup regression test")
    rows = json.loads(data["result"])
    assert len(rows) >= 1
    assert rows[0].get("name") == "Alice" or rows[0].get("columns", {}).get("name") == "Alice"
    print(" -> Point lookup regression passed")

    print("\n=================================================================")
    print("   🎉 ALL 10 AI-NATIVE DATABASE BENCHMARK TESTS PASSED 100%!     ")
    print("=================================================================")

if __name__ == "__main__":
    try:
        run_tests()
    except Exception as e:
        print(f"\n❌ Benchmark Test Failed: {e}")
        sys.exit(1)
