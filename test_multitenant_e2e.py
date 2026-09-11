import os
import sys
import time
import json
import shutil
import subprocess
import requests

SERVER_PORT = 4660
BASE_URL = f"http://127.0.0.1:{SERVER_PORT}"
DATA_DIR = "data_multitenant_test"

def clean_data():
    if os.path.exists(DATA_DIR):
        shutil.rmtree(DATA_DIR)

def start_server():
    clean_data()
    env = os.environ.copy()
    env["RUST_LOG"] = "warn"
    proc = subprocess.Popen(["./target/debug/pingdart-core", "--port", str(SERVER_PORT), "--data-dir", DATA_DIR], env=env)
    
    # Wait for server to become responsive
    for _ in range(30):
        try:
            r = requests.get(f"{BASE_URL}/", timeout=1)
            if r.status_code == 200:
                print("✅ PingDartDB Server started and responsive on port", SERVER_PORT)
                return proc
        except Exception:
            time.sleep(0.3)
    
    proc.kill()
    raise RuntimeError("Failed to start PingDartDB server")

def run_tests():
    proc = start_server()
    try:
        # 1. Login as admin to obtain auth token
        print("\n--- Test 1: Authentication ---")
        login_res = requests.post(f"{BASE_URL}/login", json={
            "username": "admin",
            "password": "admin123"
        }).json()
        assert login_res["status"] == "success", f"Login failed: {login_res}"
        token = login_res["token"]
        print("✅ Logged in successfully. Token acquired.")

        # 2. Tenant Provisioning via /tenant/create
        print("\n--- Test 2: Tenant Provisioning ---")
        # Tenant Alpha: Limited (max_vectors: 3, max_qps: 5)
        res_a = requests.post(f"{BASE_URL}/tenant/create", json={
            "token": token,
            "tenant_id": "tenant_alpha",
            "name": "Alpha Corp",
            "max_storage_mb": 1,
            "max_vectors": 3,
            "max_qps": 5
        }).json()
        assert res_a["status"] == "success", f"Alpha creation failed: {res_a}"

        # Tenant Beta: Higher limits
        res_b = requests.post(f"{BASE_URL}/tenant/create", json={
            "token": token,
            "tenant_id": "tenant_beta",
            "name": "Beta Inc",
            "max_storage_mb": 100,
            "max_vectors": 1000,
            "max_qps": 1000
        }).json()
        assert res_b["status"] == "success", f"Beta creation failed: {res_b}"

        # List tenants
        list_res = requests.post(f"{BASE_URL}/tenant/list", json={"token": token}).json()
        assert list_res["status"] == "success", f"Tenant list failed: {list_res}"
        tenants = json.loads(list_res["result"])
        tenant_ids = [t["id"] for t in tenants]
        assert "tenant_alpha" in tenant_ids and "tenant_beta" in tenant_ids, f"Tenants not in list: {tenant_ids}"
        print(f"✅ Created tenants: {tenant_ids}")

        # 3. Row-Level Security (RLS) Zero-Leak Isolation
        print("\n--- Test 3: Row-Level Security (RLS) Zero-Leak Policy ---")
        # Create shared multi-tenant table
        q_create = requests.post(f"{BASE_URL}/query", json={
            "token": token,
            "query": "CREATE TABLE org_docs (id INT PRIMARY KEY, tenant_id TEXT, title TEXT, secret_info TEXT);"
        }).json()
        assert q_create["status"] == "success", f"Create table failed: {q_create}"

        # Enable RLS on org_docs
        q_rls = requests.post(f"{BASE_URL}/query", json={
            "token": token,
            "query": "ALTER TABLE org_docs ENABLE ROW LEVEL SECURITY;"
        }).json()
        assert q_rls["status"] == "success", f"Enable RLS failed: {q_rls}"

        # Create policy: tenant_id = CURRENT_TENANT()
        q_policy = requests.post(f"{BASE_URL}/query", json={
            "token": token,
            "query": "CREATE POLICY tenant_isolation ON org_docs USING (tenant_id = CURRENT_TENANT());"
        }).json()
        assert q_policy["status"] == "success", f"Create policy failed: {q_policy}"

        # Insert Alpha's records
        ins_a1 = requests.post(f"{BASE_URL}/query", json={
            "token": token,
            "tenant_id": "tenant_alpha",
            "query": "INSERT INTO org_docs (id, tenant_id, title, secret_info) VALUES (1, 'tenant_alpha', 'Alpha Secret Plan', 'Launch Q4');"
        }).json()
        print("INSERT A1:", ins_a1)
        ins_a2 = requests.post(f"{BASE_URL}/query", json={
            "token": token,
            "tenant_id": "tenant_alpha",
            "query": "INSERT INTO org_docs (id, tenant_id, title, secret_info) VALUES (2, 'tenant_alpha', 'Alpha Financials', 'Confidential');"
        }).json()
        print("INSERT A2:", ins_a2)

        # Insert Beta's records
        requests.post(f"{BASE_URL}/query", json={
            "token": token,
            "tenant_id": "tenant_beta",
            "query": "INSERT INTO org_docs (id, tenant_id, title, secret_info) VALUES (3, 'tenant_beta', 'Beta Patent', 'Top Secret');"
        })

        # Query as Tenant Alpha -> MUST ONLY see Alpha's records
        q_alpha = requests.post(f"{BASE_URL}/query", json={
            "token": token,
            "tenant_id": "tenant_alpha",
            "query": "SELECT * FROM org_docs;"
        }).json()
        assert q_alpha["status"] == "success", f"Alpha query failed: {q_alpha}"
        alpha_rows = json.loads(q_alpha["result"])
        assert len(alpha_rows) == 2, f"Expected 2 rows for Alpha, got: {alpha_rows}"
        for r in alpha_rows:
            assert r["tenant_id"] == "tenant_alpha", f"Data leak! Alpha saw non-alpha row: {r}"
            assert "Beta" not in r["title"] and "Beta" not in r["secret_info"]
        print("✅ Tenant Alpha strictly sees ONLY its 2 records. Beta data is invisible.")

        # Query as Tenant Beta -> MUST ONLY see Beta's record
        q_beta = requests.post(f"{BASE_URL}/query", json={
            "token": token,
            "tenant_id": "tenant_beta",
            "query": "SELECT * FROM org_docs;"
        }).json()
        assert q_beta["status"] == "success", f"Beta query failed: {q_beta}"
        beta_rows = json.loads(q_beta["result"])
        assert len(beta_rows) == 1, f"Expected 1 row for Beta, got: {beta_rows}"
        assert beta_rows[0]["tenant_id"] == "tenant_beta"
        assert str(beta_rows[0]["id"]) == "3"
        assert "Alpha" not in beta_rows[0]["title"]
        print("✅ Tenant Beta strictly sees ONLY its 1 record. Alpha data is invisible.")

        # Allow sliding-window rate limit to refresh before quota test
        time.sleep(1.2)

        # 4. Quota Enforcement
        print("\n--- Test 4: Multi-Tenant Quota Enforcement ---")
        # Create vector table
        ct_res = requests.post(f"{BASE_URL}/query", json={
            "token": token,
            "query": "CREATE TABLE alpha_vectors (id INT PRIMARY KEY, emb VECTOR(384));"
        }).json()
        assert ct_res["status"] == "success", f"Create vector table failed: {ct_res}"

        # Tenant Alpha has max_vectors = 3
        dummy_vec = str([0.01 * (j % 10) for j in range(384)])
        # Insert 3 vectors (should succeed)
        for i in range(1, 4):
            res = requests.post(f"{BASE_URL}/query", json={
                "token": token,
                "tenant_id": "tenant_alpha",
                "query": f"INSERT INTO alpha_vectors (id, emb) VALUES ({i}, '{dummy_vec}');"
            }).json()
            assert res["status"] == "success", f"Insert {i} should succeed: {res}"
        print("✅ Successfully inserted 3 vectors up to Tenant Alpha's quota.")

        # 4th insertion must fail with Quota Exceeded
        res_overflow = requests.post(f"{BASE_URL}/query", json={
            "token": token,
            "tenant_id": "tenant_alpha",
            "query": f"INSERT INTO alpha_vectors (id, emb) VALUES (4, '{dummy_vec}');"
        }).json()
        assert res_overflow["status"] == "error", f"Overflow should have failed: {res_overflow}"
        assert "exceeded" in res_overflow["error"].lower() or "quota" in res_overflow["error"].lower(), f"Unexpected error: {res_overflow}"
        print(f"✅ Quota properly enforced! 4th vector insertion rejected: {res_overflow['error']}")

        # Allow sliding window to refresh before burst rate limit test
        time.sleep(1.2)

        # 5. Rate Limiting (QPS)
        print("\n--- Test 5: Sliding-Window Rate Limiting (QPS) ---")
        # Tenant Alpha has max_qps = 5
        hit_429 = False
        for i in range(15):
            res = requests.post(f"{BASE_URL}/query", json={
                "token": token,
                "tenant_id": "tenant_alpha",
                "query": "SELECT * FROM org_docs;"
            })
            if res.status_code == 429 or "rate limit" in res.text.lower():
                hit_429 = True
                print(f"✅ Received 429 Too Many Requests on iteration {i+1} as expected!")
                break
        assert hit_429, "Rate limiter did not trigger 429 on rapid burst"

        # 6. Native Model Context Protocol (MCP) Server Test
        print("\n--- Test 6: Native MCP Server Protocol Integration ---")
        mcp_env = os.environ.copy()
        mcp_env["PINGDART_URL"] = BASE_URL
        mcp_env["PINGDART_TOKEN"] = token
        mcp_proc = subprocess.Popen(
            ["./target/debug/pingdart-mcp"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=mcp_env
        )

        def send_mcp(req_obj):
            mcp_proc.stdin.write(json.dumps(req_obj) + "\n")
            mcp_proc.stdin.flush()
            line = mcp_proc.stdout.readline()
            return json.loads(line)

        # 6.1 Initialize
        init_resp = send_mcp({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {"protocolVersion": "2024-11-05"}
        })
        assert init_resp["result"]["serverInfo"]["name"] == "pingdart-mcp"
        print("✅ MCP 'initialize' handshake passed:", init_resp["result"]["serverInfo"])

        # 6.2 Tools List
        tools_resp = send_mcp({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        })
        tool_names = [t["name"] for t in tools_resp["result"]["tools"]]
        expected_tools = ["sql_query", "semantic_search", "memory_store", "memory_recall", "graph_neighbors"]
        for et in expected_tools:
            assert et in tool_names, f"Expected MCP tool {et} missing from: {tool_names}"
        print(f"✅ MCP 'tools/list' exposed all 5 primitives: {tool_names}")

        # 6.3 Tool Call: sql_query with tenant_id
        sql_call_resp = send_mcp({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "sql_query",
                "arguments": {
                    "query": "SELECT id, title FROM org_docs WHERE id = 3;",
                    "tenant_id": "tenant_beta"
                }
            }
        })
        assert not sql_call_resp["result"]["isError"], f"MCP SQL call failed: {sql_call_resp}"
        call_text = sql_call_resp["result"]["content"][0]["text"]
        assert "Beta Patent" in call_text, f"Expected Beta Patent in MCP response, got: {call_text}"
        print(f"✅ MCP 'sql_query' tool executed successfully via JSON-RPC: {call_text.strip()}")

        # 6.4 Tool Call: memory_store and memory_recall
        store_resp = send_mcp({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "memory_store",
                "arguments": {
                    "content": "User prefers concise executive summaries in dark mode.",
                    "memory_type": "Episodic",
                    "session_id": "agent_session_777",
                    "importance": 0.95
                }
            }
        })
        assert not store_resp["result"]["isError"], f"MCP memory_store failed: {store_resp}"
        print("✅ MCP 'memory_store' tool stored cognitive memory")

        recall_resp = send_mcp({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "memory_recall",
                "arguments": {
                    "session_id": "agent_session_777",
                    "limit": 5
                }
            }
        })
        assert not recall_resp["result"]["isError"], f"MCP memory_recall failed: {recall_resp}"
        recall_text = recall_resp["result"]["content"][0]["text"]
        assert "executive summaries" in recall_text
        print(f"✅ MCP 'memory_recall' retrieved stored memory: {recall_text.strip()}")

        mcp_proc.terminate()
        print("\n========================================================")
        print("🎉 ALL MULTI-TENANT, RLS, QUOTA, RATE LIMIT & MCP TESTS PASSED!")
        print("========================================================")

    finally:
        proc.terminate()
        proc.wait()
        clean_data()

if __name__ == "__main__":
    run_tests()
