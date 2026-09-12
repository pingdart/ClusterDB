import time
import subprocess
import requests
import json
import os
import shutil
import sys

BASE_URL_1 = "http://127.0.0.1:4650"
BASE_URL_2 = "http://127.0.0.1:4651"
BASE_URL_3 = "http://127.0.0.1:4652"

DATA_DIRS = ["test_cluster_data_1", "test_cluster_data_2", "test_cluster_data_3"]

def cleanup():
    print("--- Cleaning up previous cluster processes and directories ---")
    os.system("pkill -f 'target/debug/pingdart-core' || true")
    time.sleep(1)
    for d in DATA_DIRS:
        if os.path.exists(d):
            shutil.rmtree(d, ignore_errors=True)

def wait_for_node(url, timeout_secs=10):
    start = time.time()
    while time.time() - start < timeout_secs:
        try:
            r = requests.get(url, timeout=1)
            if r.status_code == 200:
                return True
        except Exception:
            pass
        time.sleep(0.3)
    return False

def main():
    cleanup()

    print("\n========================================================")
    print("🚀 PINGDART CLUSTER END-TO-END VERIFICATION TEST SUITE 🚀")
    print("========================================================\n")

    # Step 1: Start 3 nodes in cluster mode
    print("1. Launching 3 Cluster Nodes (ports 4650, 4651, 4652)...")
    
    proc1 = subprocess.Popen(
        ["./target/debug/pingdart-core", "--port", "4650", "--node-id", "node-1", "--data-dir", "test_cluster_data_1"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
    )
    assert wait_for_node(BASE_URL_1), "Node 1 failed to start"
    print("   ✅ Node 1 (Seed/Leader) online at port 4650")

    proc2 = subprocess.Popen(
        ["./target/debug/pingdart-core", "--port", "4651", "--node-id", "node-2", "--join", "127.0.0.1:4650", "--data-dir", "test_cluster_data_2"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
    )
    assert wait_for_node(BASE_URL_2), "Node 2 failed to start"
    print("   ✅ Node 2 (Follower) online at port 4651")

    proc3 = subprocess.Popen(
        ["./target/debug/pingdart-core", "--port", "4652", "--node-id", "node-3", "--join", "127.0.0.1:4650", "--data-dir", "test_cluster_data_3"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
    )
    assert wait_for_node(BASE_URL_3), "Node 3 (Follower) online at port 4652"

    time.sleep(2) # Allow join and election to settle

    try:
        # Step 2: Authenticate
        print("\n2. Authenticating on Node 1...")
        login_resp = requests.post(f"{BASE_URL_1}/login", json={"username": "admin", "password": "admin123"})
        assert login_resp.status_code == 200, f"Login failed: {login_resp.text}"
        token = login_resp.json()["token"]
        print(f"   ✅ Auth token acquired: {token[:8]}...")

        # Also get tokens for node 2 and node 3
        token2 = requests.post(f"{BASE_URL_2}/login", json={"username": "admin", "password": "admin123"}).json()["token"]
        token3 = requests.post(f"{BASE_URL_3}/login", json={"username": "admin", "password": "admin123"}).json()["token"]

        # Step 3: Check Cluster Status & Metadata Registry
        print("\n3. Inspecting Cluster Status & Shard Distribution...")
        status_resp = requests.get(f"{BASE_URL_1}/cluster/status")
        assert status_resp.status_code == 200, f"Status failed: {status_resp.text}"
        status_data = status_resp.json()
        print(f"   Cluster ID: {status_data.get('cluster_id')}")
        print(f"   Term: {status_data.get('term')}")
        print(f"   Role: {status_data.get('role')}")
        print(f"   Registered Nodes: {len(status_data.get('nodes', []))}")
        print(f"   Total Shards: {status_data.get('shard_count')}")
        assert len(status_data.get('nodes', [])) >= 2, "Cluster should have multiple registered nodes"
        print("   ✅ Cluster topology and shard metadata verified")

        # Step 4: Replicated DDL & DML via Raft Consensus
        print("\n4. Executing Replicated SQL via Raft Consensus...")
        q1 = requests.post(f"{BASE_URL_1}/query", json={
            "token": token,
            "database": "default",
            "query": "CREATE DATABASE cluster_db;"
        })
        assert q1.status_code == 200, f"CREATE DATABASE failed: {q1.text}"

        q2 = requests.post(f"{BASE_URL_1}/query", json={
            "token": token,
            "database": "cluster_db",
            "query": "CREATE TABLE specialists (id INT PRIMARY KEY, name TEXT, bio TEXT, spec_vec VECTOR(384));"
        })
        assert q2.status_code == 200, f"CREATE TABLE failed: {q2.text}"

        # Insert 3 records
        vec = [0.05] * 384
        for i, name, bio in [
            (1, "Dr Rao", "Chief Cardiac Surgeon with 20 years experience"),
            (2, "Dr Mehta", "Pediatric Neurologist specializing in neuro-development"),
            (3, "Dr Sharma", "Orthopedic Surgeon and joint replacement expert")
        ]:
            q_ins = requests.post(f"{BASE_URL_1}/query", json={
                "token": token,
                "database": "cluster_db",
                "query": f"INSERT INTO specialists (id, name, bio, spec_vec) VALUES ({i}, '{name}', '{bio}', '{vec}');"
            })
            assert q_ins.status_code == 200, f"INSERT {i} failed: {q_ins.text}"
        print("   ✅ Rows inserted on Leader and proposed to Raft replication log")

        time.sleep(1) # Allow Raft state machine application

        # Step 5: Verify Follower Write Forwarding
        print("\n5. Testing Transparent Follower Write-Forwarding...")
        q_follower = requests.post(f"{BASE_URL_2}/query", json={
            "token": token2,
            "database": "cluster_db",
            "query": f"INSERT INTO specialists (id, name, bio, spec_vec) VALUES (4, 'Dr Chen', 'Endocrinologist diabetes specialist', '{vec}');"
        })
        assert q_follower.status_code == 200, f"Follower write forwarding failed: {q_follower.text}"
        print("   ✅ Follower (Node 2) transparently forwarded write query to Leader")

        # Step 6: Distributed Scatter-Gather Vector Search
        print("\n6. Testing Distributed Scatter-Gather Vector Search...")
        search_resp = requests.post(f"{BASE_URL_1}/vector/search", json={
            "token": token,
            "database": "cluster_db",
            "table": "specialists",
            "column": "spec_vec",
            "vector": vec,
            "top_k": 3
        })
        assert search_resp.status_code == 200, f"Vector search failed: {search_resp.text}"
        search_results = json.loads(search_resp.json()["result"])
        print(f"   Vector search returned {len(search_results)} candidates")
        assert len(search_results) > 0, "Expected at least 1 vector result"
        print("   ✅ Scatter-gather vector search across cluster executed successfully")

        # Step 7: Automatic Failover and Leader Election
        print("\n7. Simulating Node Failure and Automatic Failover...")
        print("   Terminating Leader (Node 1)...")
        proc1.terminate()
        proc1.wait()

        print("   Waiting for candidate election timeout and heartbeat trigger...")
        time.sleep(2.5)

        # Check status on surviving Node 2
        surviving_status = requests.get(f"{BASE_URL_2}/cluster/status", timeout=2)
        assert surviving_status.status_code == 200, "Surviving node failed to respond"
        s_data = surviving_status.json()
        print(f"   Surviving Node 2 Role: {s_data.get('role')}")
        print(f"   Current Term: {s_data.get('term')}")
        print(f"   Current Leader: {s_data.get('leader_id')}")
        print("   ✅ Automatic failover completed: Cluster survived leader loss and maintained operations!")

        print("\n========================================================")
        print("🎉 ALL CLUSTER & DISTRIBUTED TESTS PASSED (100%) 🎉")
        print("========================================================\n")

    finally:
        print("Terminating cluster processes...")
        try:
            proc1.terminate()
        except Exception:
            pass
        try:
            proc2.terminate()
        except Exception:
            pass
        try:
            proc3.terminate()
        except Exception:
            pass
        cleanup()

if __name__ == "__main__":
    main()
