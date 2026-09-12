import os
import sys
import time
import json
import subprocess

def run_benchmark_suite():
    print("==================================================================")
    print("🧪 Phase 11B: Scale Benchmark Harness & Performance Telemetry")
    print("==================================================================\n")

    json_out = "benchmark_results.json"
    md_out = "benchmark_results.md"
    
    if os.path.exists(json_out):
        os.remove(json_out)
    if os.path.exists(md_out):
        os.remove(md_out)

    print("--- Step 1: Compiling pingdart-benchmark Binary (Release Mode) ---")
    build_proc = subprocess.run(["cargo", "build", "--bin", "pingdart-benchmark", "--release"], capture_output=True, text=True)
    assert build_proc.returncode == 0, f"Failed to build benchmark binary:\n{build_proc.stderr}"
    print("✅ pingdart-benchmark compiled successfully.")

    print("\n--- Step 2: Executing Benchmark Harness (Scale 10k, Concurrency 8) ---")
    cmd = [
        "./target/release/pingdart-benchmark",
        "--scale", "10k",
        "--concurrency", "8",
        "--output-json", json_out,
        "--output-md", md_out
    ]
    
    start_time = time.time()
    bench_proc = subprocess.run(cmd, capture_output=True, text=True)
    elapsed = time.time() - start_time
    
    print(bench_proc.stdout)
    if bench_proc.returncode != 0:
        print(f"STDERR:\n{bench_proc.stderr}")
        raise RuntimeError("Benchmark binary failed execution")

    print(f"✅ Benchmark run completed in {elapsed:.2f} seconds.")

    print("\n--- Step 3: Validating Benchmark Telemetry & Percentiles ---")
    assert os.path.exists(json_out), f"Missing output JSON: {json_out}"
    assert os.path.exists(md_out), f"Missing output Markdown: {md_out}"

    with open(json_out, "r") as f:
        data = json.load(f)

    # 1. OLTP Inserts validation
    inserts = data["oltp_inserts"]
    print(f"OLTP Inserts: {inserts['successful_operations']} ops | QPS: {inserts['qps']:.1f} | P50: {inserts['percentiles']['p50_ms']:.3f}ms | P99: {inserts['percentiles']['p99_ms']:.3f}ms")
    assert inserts["successful_operations"] == 10000
    assert inserts["qps"] > 100.0
    p = inserts["percentiles"]
    assert p["min_ms"] <= p["p50_ms"] <= p["p90_ms"] <= p["p95_ms"] <= p["p99_ms"] <= p["max_ms"]

    # 2. OLTP Lookups validation
    lookups = data["oltp_lookups"]
    print(f"OLTP Lookups: {lookups['successful_operations']} ops | QPS: {lookups['qps']:.1f} | P50: {lookups['percentiles']['p50_ms']:.3f}ms | P99: {lookups['percentiles']['p99_ms']:.3f}ms")
    assert lookups["successful_operations"] >= 500
    assert lookups["qps"] > 100.0
    lp = lookups["percentiles"]
    assert lp["min_ms"] <= lp["p50_ms"] <= lp["p90_ms"] <= lp["p95_ms"] <= lp["p99_ms"] <= lp["max_ms"]

    # 3. Vector HNSW Search validation
    vec = data["vector_hnsw"]
    print(f"Vector HNSW Search: {vec['successful_operations']} ops | QPS: {vec['qps']:.1f} | P50: {vec['percentiles']['p50_ms']:.3f}ms | P99: {vec['percentiles']['p99_ms']:.3f}ms")
    assert vec["successful_operations"] >= 50
    assert vec["qps"] > 0.0

    # 4. Columnar OLAP validation
    olap = data["columnar_olap"]
    print(f"Vectorized OLAP: {olap['successful_operations']} ops | QPS: {olap['qps']:.1f} | P50: {olap['percentiles']['p50_ms']:.3f}ms | P99: {olap['percentiles']['p99_ms']:.3f}ms")
    assert olap["successful_operations"] >= 30

    # 5. Storage Stats validation
    storage = data["storage"]
    print(f"Storage Footprint: {storage['total_disk_mb']:.2f} MB | Estimated WAF: {storage['estimated_waf']:.2f}x")
    assert storage["total_disk_bytes"] > 0

    # 6. Calibrated CBO Cost Weights validation
    weights = data["calibrated_weights"]
    print("\n🧠 Calibrated Cost-Based Optimizer (CBO) Weights Extracted:")
    print(f"   • W_io (SeqScan per 1k rows): {weights['cost_per_1k_seq_scan_ms']:.6f} ms")
    print(f"   • W_btree (Point Lookup):     {weights['cost_btree_lookup_ms']:.6f} ms")
    print(f"   • W_vec (Distance Calc):      {weights['cost_vector_dist_calc_ms']:.6f} ms")
    print(f"   • W_agg (Columnar Chunk Agg): {weights['cost_columnar_agg_chunk_ms']:.6f} ms")
    print(f"   • W_net (Network Hop RPC):    {weights['cost_network_hop_ms']:.6f} ms")

    assert weights["cost_per_1k_seq_scan_ms"] > 0.0
    assert weights["cost_btree_lookup_ms"] > 0.0
    assert weights["cost_vector_dist_calc_ms"] > 0.0
    assert weights["cost_columnar_agg_chunk_ms"] > 0.0
    assert weights["cost_network_hop_ms"] > 0.0

    print("\n🎉 ALL PHASE 11B SCALE BENCHMARK HARNESS TESTS PASSED PERFECTLY! 🎉\n")

if __name__ == "__main__":
    run_benchmark_suite()
