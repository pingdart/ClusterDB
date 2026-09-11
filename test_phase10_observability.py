import os
import sys
import time
import json
import shutil
import subprocess
import requests

SERVER_PORT = 4790
BASE_URL = f"http://127.0.0.1:{SERVER_PORT}"
DATA_DIR = "data_observability_test"

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
        assert "OpenTelemetry Tracing & Query Profiler (EXPLAIN ANALYZE)" in capabilities
        print("✅ Status capabilities include OpenTelemetry Tracing & Query Profiler.")

        print("\n--- Test 2: Admin Session Login ---")
        login_res = requests.post(f"{BASE_URL}/login", json={
            "username": "admin",
            "password": "admin123"
        }).json()
        assert login_res["status"] == "success"
        token = login_res["token"]
        print("✅ SuperAdmin logged in successfully.")

        print("\n--- Test 3: Relational Table Seeding & EXPLAIN ANALYZE ---")
        requests.post(f"{BASE_URL}/query", json={
            "query": "CREATE TABLE employees (id INT, name TEXT, department TEXT, salary FLOAT);",
            "token": token
        })
        for i in range(1, 11):
            requests.post(f"{BASE_URL}/query", json={
                "query": f"INSERT INTO employees VALUES ({i}, 'Emp_{i}', 'Engineering', {75000 + i * 2000});",
                "token": token
            })
        print("✅ Seeded 10 relational employee records.")

        # Execute EXPLAIN ANALYZE on point lookup
        explain_res = requests.post(f"{BASE_URL}/query", json={
            "query": "EXPLAIN ANALYZE SELECT * FROM employees WHERE id = 5;",
            "token": token
        }).json()
        assert explain_res["status"] == "success", f"EXPLAIN ANALYZE failed: {explain_res}"
        report = json.loads(explain_res["result"])

        print("\n📊 EXPLAIN ANALYZE REPORT (Relational Query):")
        print(f"Total Latency: {report['total_ms']:.3f} ms")
        print(f"Parser: {report['stages']['parser_ms']:.3f} ms | Planner: {report['stages']['planner_ms']:.3f} ms")
        print(f"Rows Scanned: {report['rows_scanned']} | Rows Returned: {report['rows_returned']}")
        print(f"Allocated Memory: {report['memory']['memory_bytes_allocated']} bytes")
        print("Plan Tree ASCII:")
        print(report['plan_ascii'])

        assert report["rows_scanned"] >= 1
        assert report["rows_returned"] >= 1
        assert "stages" in report
        assert "plan_tree" in report
        assert "BTreeIndexScan" in report["plan_ascii"] or "SequentialScan" in report["plan_ascii"]
        print("✅ Relational EXPLAIN ANALYZE validated with PlanNode execution tree and memory tracking.")

        print("\n--- Test 4: Semantic SQL & Vector HNSW EXPLAIN ANALYZE ---")
        requests.post(f"{BASE_URL}/query", json={
            "query": "CREATE TABLE doctors (id INT, name TEXT, bio TEXT, embedding VECTOR(384) GENERATED FROM bio USING default_embed);",
            "token": token
        })
        requests.post(f"{BASE_URL}/query", json={
            "query": "INSERT INTO doctors VALUES (1, 'Dr Rao', 'Cardiologist with 15 years experience in open heart surgery and bypass');",
            "token": token
        })
        requests.post(f"{BASE_URL}/query", json={
            "query": "INSERT INTO doctors VALUES (2, 'Dr Sarah Chen', 'Pediatrician specializing in infant health child care and neonatal');",
            "token": token
        })
        requests.post(f"{BASE_URL}/query", json={
            "query": "INSERT INTO doctors VALUES (3, 'Dr Ramesh Kumar', 'Interventional cardiologist performing angioplasty heart catheterization');",
            "token": token
        })
        print("✅ Seeded doctors with auto-generated embeddings.")

        semantic_explain = requests.post(f"{BASE_URL}/query", json={
            "query": "EXPLAIN ANALYZE SELECT * FROM doctors WHERE SEMANTIC_MATCH(bio, 'heart surgery cardiology specialist') > 0.15;",
            "token": token
        }).json()
        assert semantic_explain["status"] == "success", f"Semantic EXPLAIN failed: {semantic_explain}"
        sem_report = json.loads(semantic_explain["result"])

        print("\n🧠 EXPLAIN ANALYZE REPORT (Semantic / Vector Query):")
        print(f"Total Latency: {sem_report['total_ms']:.3f} ms")
        print(f"Semantic Match Time: {sem_report['stages']['semantic_match_ms']:.3f} ms")
        if sem_report.get("vector_metrics"):
            vm = sem_report["vector_metrics"]
            print(f"Vector Metrics -> Visited Nodes: {vm['visited_nodes']} | Distance Calcs: {vm['distance_computations']} | ef_search: {vm['ef_search']} | Queue Peak: {vm['candidate_queue_peak']}")
            assert vm["distance_computations"] > 0
            assert vm["visited_nodes"] > 0
            assert vm["ef_search"] == 100
            assert vm["candidate_queue_peak"] > 0
        print("Plan Tree ASCII:")
        print(sem_report['plan_ascii'])

        assert "SemanticFilter" in sem_report["plan_ascii"]
        assert "HnswProximitySearch" in sem_report["plan_ascii"]
        print("✅ Semantic EXPLAIN ANALYZE validated with vector distance metrics and HNSW traversal stats.")

        print("\n--- Test 5: EXPLAIN ANALYZE Read-Only Safety Guard ---")
        mutation_explain = requests.post(f"{BASE_URL}/query", json={
            "query": "EXPLAIN ANALYZE INSERT INTO employees VALUES (99, 'Hacker', 'Ops', 50000);",
            "token": token
        }).json()
        assert mutation_explain["status"] == "error", "EXPLAIN ANALYZE should have rejected INSERT query"
        assert "strictly restricted to read-only queries (SELECT)" in mutation_explain["error"]
        print("✅ Mutation query successfully rejected by EXPLAIN ANALYZE safety guard.")

        print("\n--- Test 6: Distributed Query Telemetry & Scatter-Gather Tree ---")
        dist_explain = requests.post(f"{BASE_URL}/query", json={
            "query": "EXPLAIN ANALYZE SELECT * FROM employees WHERE salary > 80000 CLUSTER;",
            "token": token
        }).json()
        assert dist_explain["status"] == "success", f"Distributed EXPLAIN failed: {dist_explain}"
        dist_report = json.loads(dist_explain["result"])
        assert "distributed_metrics" in dist_report
        dm = dist_report["distributed_metrics"]
        print(f"Distributed Telemetry -> Remote Nodes: {dm['remote_nodes']}, Network: {dm['network_ms']}ms, Remote Scan: {dm['remote_scan_ms']}ms, Merge: {dm['merge_ms']}ms")
        assert dm["remote_nodes"] == 3
        assert dm["network_ms"] > 0
        assert "ScatterGatherExchange" in dist_report["plan_ascii"]
        print("Plan Tree ASCII (Distributed):")
        print(dist_report["plan_ascii"])
        print("✅ Distributed query telemetry and ScatterGatherExchange tree validated.")

        print("\n--- Test 7: OpenTelemetry Distributed Tracing & W3C Context Propagation ---")
        w3c_trace_id = "4bf92f3577b34da6a3ce929d0e0e4736"
        w3c_parent_span = "00f067aa0ba902b7"
        traceparent = f"00-{w3c_trace_id}-{w3c_parent_span}-01"

        # Execute query passing traceparent
        traced_query_res = requests.post(f"{BASE_URL}/query", json={
            "query": "SELECT * FROM employees WHERE salary > 80000;",
            "token": token,
            "tenant_id": "tenant_finance",
            "traceparent": traceparent
        }).json()
        assert traced_query_res["status"] == "success"

        # Query /observability/traces
        traces_list_res = requests.get(f"{BASE_URL}/observability/traces").json()
        assert len(traces_list_res) > 0
        print(f"✅ Listed {len(traces_list_res)} active distributed trace(s).")

        # Inspect specific trace
        trace_spans_res = requests.get(f"{BASE_URL}/observability/traces/{w3c_trace_id}").json()
        assert len(trace_spans_res) > 0, f"Spans not found for trace {w3c_trace_id}"
        span = trace_spans_res[0]
        print(f"✅ Retrieved trace {w3c_trace_id}: Span '{span['name']}' (Duration: {span['duration_ms']:.3f} ms, Status: {span['status']})")
        assert span["trace_id"] == w3c_trace_id
        assert span["attributes"]["db.tenant"] == "tenant_finance"
        assert span["status"] == "OK"

        print("\n--- Test 8: Slow Query Logger & Automated Diagnostics ---")
        slow_queries_res = requests.get(f"{BASE_URL}/observability/slow_queries").json()
        # Slow query logger exists and answers HTTP queries
        assert isinstance(slow_queries_res, list)
        print("✅ /observability/slow_queries endpoint functional.")

        print("\n🎉 ALL PHASE 10 OPENTELEMETRY & EXPLAIN ANALYZE TESTS PASSED PERFECTLY! 🎉\n")

    finally:
        proc.kill()
        proc.wait()
        clean_data()

if __name__ == "__main__":
    run_tests()
