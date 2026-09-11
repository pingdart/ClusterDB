import os
import sys
import time
import json
import shutil
import subprocess

def run_tests():
    print("\n============================================================")
    print("🚀 PingDartDB Phase 11C Scale Validation Suite (1M-5M Scale)")
    print("============================================================\n")

    bench_bin = "./target/debug/pingdart-benchmark"
    assert os.path.exists(bench_bin), f"Benchmark binary not found at {bench_bin}. Run cargo build first."

    print("--- Test 1: 1,000,000-Row Scale Benchmark with JSON Output ---")
    out_json = "bench_scale_1m_results.json"
    out_md = "bench_scale_1m_results.md"
    for f in [out_json, out_md]:
        if os.path.exists(f):
            os.remove(f)

    start_time = time.time()
    cmd = [bench_bin, "--scale", "1m", "--output-json", out_json, "--output-md", out_md]
    res = subprocess.run(cmd, capture_output=True, text=True)
    assert res.returncode == 0, f"Benchmark failed with code {res.returncode}:\n{res.stderr}"

    print(f"✅ Benchmark executed in {time.time() - start_time:.2f}s")
    assert os.path.exists(out_json), f"Expected JSON report {out_json} not created"
    assert os.path.exists(out_md), f"Expected Markdown report {out_md} not created"

    with open(out_json, "r") as f:
        report = json.load(f)

    print("\n--- Test 2: Ingestion Throughput Verification ---")
    rows = report["total_rows"]
    qps = report["ingestion_rows_per_sec"]
    elapsed = report["ingestion_elapsed_secs"]
    assert rows == 1_000_000, f"Expected 1,000,000 rows, got {rows}"
    assert qps > 40_000, f"Ingestion QPS ({qps}) below acceptable threshold (40,000 rows/sec)"
    print(f"✅ Ingestion throughput verified: {qps:,.0f} rows/sec ({rows:,} rows in {elapsed:.2f}s)")

    print("\n--- Test 3: Vectorized Columnar Aggregations ---")
    full_agg = report["full_aggregation"]
    assert full_agg["successful_operations"] == 10
    assert full_agg["percentiles"]["mean_ms"] < 2500.0, f"Aggregation too slow: {full_agg['percentiles']['mean_ms']}ms"
    print(f"✅ Full Aggregation (COUNT, SUM, AVG, MIN, MAX) verified: {full_agg['percentiles']['mean_ms']:.2f}ms (P99: {full_agg['percentiles']['p99_ms']:.2f}ms)")

    grp_agg = report["group_by_aggregation"]
    assert grp_agg["successful_operations"] == 10
    print(f"✅ Group-By Aggregation verified: {grp_agg['percentiles']['mean_ms']:.2f}ms (P99: {grp_agg['percentiles']['p99_ms']:.2f}ms)")

    print("\n--- Test 4: Zone-Map Chunk Pruning Speedup Validation ---")
    pruned = report["pruned_range_scan"]
    unpruned = report["unpruned_full_scan"]
    speedup = report["pruning_speedup_ratio"]
    total_chunks = report["total_chunks"]
    pruned_chunks = report["estimated_pruned_chunks"]

    prune_pct = (pruned_chunks / max(1, total_chunks)) * 100.0
    print(f"  Total Chunks: {total_chunks} | Estimated Pruned: {pruned_chunks} ({prune_pct:.1f}%)")
    print(f"  Unpruned Scan Latency: {unpruned['percentiles']['mean_ms']:.2f}ms")
    print(f"  Pruned Scan Latency:   {pruned['percentiles']['mean_ms']:.2f}ms")
    print(f"  Pruning Speedup Factor: {speedup:.2f}x FASTER")

    assert speedup >= 3.0, f"Expected at least 3.0x speedup from zone maps, got {speedup:.2f}x"
    assert prune_pct >= 70.0, f"Expected at least 70% of chunks pruned, got {prune_pct:.1f}%"
    print(f"✅ Zone-Map chunk pruning validated: {speedup:.2f}x speedup with {prune_pct:.1f}% chunks pruned!")

    # Clean up generated test report files
    for f in [out_json, out_md]:
        if os.path.exists(f):
            os.remove(f)

    print("\n🎉 ALL PHASE 11C SCALE VALIDATION TESTS PASSED PERFECTLY! 🎉\n")

if __name__ == "__main__":
    run_tests()
