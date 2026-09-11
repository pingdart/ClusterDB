use anyhow::Result;
use pingdart_core::benchmark::BenchmarkRunner;
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    let mut scale_label = "10k".to_string();
    let mut total_records = 10_000;
    let mut concurrency = 8;
    let mut output_json: Option<String> = None;
    let mut output_md: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--scale" | "-s" => {
                if i + 1 < args.len() {
                    scale_label = args[i + 1].clone();
                    match scale_label.to_lowercase().as_str() {
                        "1k" => total_records = 1_000,
                        "10k" => total_records = 10_000,
                        "50k" => total_records = 50_000,
                        "100k" => total_records = 100_000,
                        "1m" => total_records = 1_000_000,
                        "5m" => total_records = 5_000_000,
                        "10m" => total_records = 10_000_000,
                        _ => total_records = scale_label.parse().unwrap_or(10_000),
                    }
                    i += 1;
                }
            }
            "--mode" => {
                if i + 1 < args.len() {
                    let m = args[i + 1].to_lowercase();
                    if m == "scale" || m == "columnar" {
                        total_records = 1_000_000;
                        scale_label = "1m".to_string();
                    }
                    i += 1;
                }
            }
            "--columnar-scale" => {
                total_records = 1_000_000;
                scale_label = "1m".to_string();
            }
            "--concurrency" | "-c" => {
                if i + 1 < args.len() {
                    concurrency = args[i + 1].parse().unwrap_or(8);
                    i += 1;
                }
            }
            "--output-json" => {
                if i + 1 < args.len() {
                    output_json = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--output-md" => {
                if i + 1 < args.len() {
                    output_md = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    if total_records >= 500_000 || scale_label.ends_with('m') || scale_label.ends_with('M') {
        println!("============================================================");
        println!("🚀 PingDartDB 10M–100M Scale Benchmark Harness (Phase 11C)");
        println!("============================================================");
        println!("• Scale Target: {} ({} records)", scale_label, total_records);
        println!("• Engine: Vectorized Columnar Segments + Zone-Map Chunk Pruning");
        println!("• Ingestion Mode: Streaming Micro-Batches (10,000 rows/batch)");
        println!("============================================================\n");

        let bench_dir = format!(".pingdart_scale_{}", scale_label);
        let _ = std::fs::remove_dir_all(&bench_dir);

        let runner = pingdart_core::benchmark::ScaleBenchmarkRunner::new(&bench_dir)?;
        let report = runner.run_scale_tier(&scale_label, total_records, 10_000)?;

        println!("\n📊 PHASE 11C SCALE BENCHMARK REPORT:");
        println!("------------------------------------------------------------");
        println!("• Ingestion Throughput: {:.0} rows/sec ({:.2}s total)", report.ingestion_rows_per_sec, report.ingestion_elapsed_secs);
        println!("• Full Table Aggregation (COUNT, SUM, AVG, MIN, MAX):");
        println!("    Mean: {:.3} ms | P50: {:.3} ms | P99: {:.3} ms",
            report.full_aggregation.percentiles.mean_ms,
            report.full_aggregation.percentiles.p50_ms,
            report.full_aggregation.percentiles.p99_ms
        );
        println!("• Group-By Aggregation (GROUP BY category):");
        println!("    Mean: {:.3} ms | P50: {:.3} ms | P99: {:.3} ms",
            report.group_by_aggregation.percentiles.mean_ms,
            report.group_by_aggregation.percentiles.p50_ms,
            report.group_by_aggregation.percentiles.p99_ms
        );
        println!("• Zone-Map Chunk Pruning Efficiency:");
        println!("    Total Chunks: {} | Chunks Pruned: {} (~{:.1}%)",
            report.total_chunks,
            report.estimated_pruned_chunks,
            (report.estimated_pruned_chunks as f64 / report.total_chunks.max(1) as f64) * 100.0
        );
        println!("    Unpruned Scan: {:.3} ms vs Pruned Scan: {:.3} ms",
            report.unpruned_full_scan.percentiles.mean_ms,
            report.pruned_range_scan.percentiles.mean_ms
        );
        println!("    🔥 Pruning Speedup Factor: {:.2}x FASTER", report.pruning_speedup_ratio);
        println!("------------------------------------------------------------");

        if let Some(json_path) = output_json {
            let json_str = serde_json::to_string_pretty(&report)?;
            std::fs::write(&json_path, json_str)?;
            println!("✅ Wrote JSON benchmark report to: {}", json_path);
        }

        if let Some(md_path) = output_md {
            let md = format!(
                "# PingDartDB Phase 11C Scale Benchmark Report: {scale}\n\n\
                - **Total Records Evaluated**: {rows}\n\
                - **Ingestion Throughput**: {ingest_qps:.0} rows/sec\n\
                - **Zone-Map Pruning Speedup**: **{speedup:.2}x FASTER**\n\n\
                | Workload | Latency Mean (ms) | P50 (ms) | P99 (ms) | QPS |\n\
                | :--- | :--- | :--- | :--- | :--- |\n\
                | Full Aggregation (COUNT, SUM, AVG, MIN, MAX) | {full_mean:.3} | {full_p50:.3} | {full_p99:.3} | {full_qps:.1} |\n\
                | Group-By Aggregation | {grp_mean:.3} | {grp_p50:.3} | {grp_p99:.3} | {grp_qps:.1} |\n\
                | Zone-Map Pruned Scan (Top 10%) | {pruned_mean:.3} | {pruned_p50:.3} | {pruned_p99:.3} | {pruned_qps:.1} |\n\
                | Unpruned Full Scan | {unpruned_mean:.3} | {unpruned_p50:.3} | {unpruned_p99:.3} | {unpruned_qps:.1} |\n",
                scale = report.scale_tier,
                rows = report.total_rows,
                ingest_qps = report.ingestion_rows_per_sec,
                speedup = report.pruning_speedup_ratio,
                full_mean = report.full_aggregation.percentiles.mean_ms,
                full_p50 = report.full_aggregation.percentiles.p50_ms,
                full_p99 = report.full_aggregation.percentiles.p99_ms,
                full_qps = report.full_aggregation.qps,
                grp_mean = report.group_by_aggregation.percentiles.mean_ms,
                grp_p50 = report.group_by_aggregation.percentiles.p50_ms,
                grp_p99 = report.group_by_aggregation.percentiles.p99_ms,
                grp_qps = report.group_by_aggregation.qps,
                pruned_mean = report.pruned_range_scan.percentiles.mean_ms,
                pruned_p50 = report.pruned_range_scan.percentiles.p50_ms,
                pruned_p99 = report.pruned_range_scan.percentiles.p99_ms,
                pruned_qps = report.pruned_range_scan.qps,
                unpruned_mean = report.unpruned_full_scan.percentiles.mean_ms,
                unpruned_p50 = report.unpruned_full_scan.percentiles.p50_ms,
                unpruned_p99 = report.unpruned_full_scan.percentiles.p99_ms,
                unpruned_qps = report.unpruned_full_scan.qps,
            );
            std::fs::write(&md_path, md)?;
            println!("✅ Wrote Markdown benchmark report to: {}", md_path);
        }

        let _ = std::fs::remove_dir_all(&bench_dir);
        return Ok(());
    }

    println!("============================================================");
    println!("🚀 PingDartDB Enterprise Scale Benchmark Harness (Phase 11B)");
    println!("============================================================");
    println!("• Scale Target: {} ({} records)", scale_label, total_records);
    println!("• Concurrency Level: {} parallel workers", concurrency);
    println!("• Storage Engine: Sled + Vector HNSW + Vectorized Columnar");
    println!("============================================================\n");

    let bench_dir = format!(".pingdart_bench_{}", scale_label);
    let _ = std::fs::remove_dir_all(&bench_dir);

    let runner = BenchmarkRunner::new(&bench_dir)?;
    let report = runner.run_full_suite(&scale_label, total_records, concurrency)?;

    println!("\n📊 BENCHMARK RESULTS SUMMARY:");
    println!("------------------------------------------------------------");
    println!("1. OLTP Concurrent Inserts:");
    println!("   • Total Ops: {}", report.oltp_inserts.successful_operations);
    println!("   • QPS / Throughput: {:.1} ops/sec", report.oltp_inserts.qps);
    println!("   • Latency P50: {:.3} ms | P95: {:.3} ms | P99: {:.3} ms", 
        report.oltp_inserts.percentiles.p50_ms,
        report.oltp_inserts.percentiles.p95_ms,
        report.oltp_inserts.percentiles.p99_ms
    );

    println!("\n2. OLTP Point Lookups:");
    println!("   • Total Ops: {}", report.oltp_lookups.successful_operations);
    println!("   • QPS / Throughput: {:.1} ops/sec", report.oltp_lookups.qps);
    println!("   • Latency P50: {:.3} ms | P95: {:.3} ms | P99: {:.3} ms",
        report.oltp_lookups.percentiles.p50_ms,
        report.oltp_lookups.percentiles.p95_ms,
        report.oltp_lookups.percentiles.p99_ms
    );

    println!("\n3. HNSW Vector Proximity Search:");
    println!("   • Total Ops: {}", report.vector_hnsw.successful_operations);
    println!("   • QPS / Throughput: {:.1} ops/sec", report.vector_hnsw.qps);
    println!("   • Latency P50: {:.3} ms | P95: {:.3} ms | P99: {:.3} ms",
        report.vector_hnsw.percentiles.p50_ms,
        report.vector_hnsw.percentiles.p95_ms,
        report.vector_hnsw.percentiles.p99_ms
    );

    println!("\n4. Vectorized Columnar OLAP:");
    println!("   • Total Ops: {}", report.columnar_olap.successful_operations);
    println!("   • QPS / Throughput: {:.1} ops/sec", report.columnar_olap.qps);
    println!("   • Latency P50: {:.3} ms | P95: {:.3} ms | P99: {:.3} ms",
        report.columnar_olap.percentiles.p50_ms,
        report.columnar_olap.percentiles.p95_ms,
        report.columnar_olap.percentiles.p99_ms
    );

    println!("\n5. Storage Telemetry:");
    println!("   • Disk Size: {:.2} MB", report.storage.total_disk_mb);
    println!("   • Estimated WAF: {:.2}x", report.storage.estimated_waf);

    println!("\n6. Calibrated CBO Cost Weights:");
    println!("   • SeqScan (per 1k rows): {:.4} ms", report.calibrated_weights.cost_per_1k_seq_scan_ms);
    println!("   • BTree Point Lookup: {:.4} ms", report.calibrated_weights.cost_btree_lookup_ms);
    println!("   • Vector Distance Calc: {:.6} ms", report.calibrated_weights.cost_vector_dist_calc_ms);
    println!("   • Columnar Chunk Agg: {:.4} ms", report.calibrated_weights.cost_columnar_agg_chunk_ms);
    println!("------------------------------------------------------------\n");

    if let Some(json_path) = output_json {
        let json_data = serde_json::to_string_pretty(&report)?;
        std::fs::write(&json_path, json_data)?;
        println!("✅ Wrote JSON benchmark report to: {}", json_path);
    }

    if let Some(md_path) = output_md {
        let md = format!(
            "# PingDartDB Scale Benchmark Report ({scale})\n\n\
            - **Total Rows**: {rows}\n\
            - **Concurrency**: {conc} workers\n\
            - **Total Elapsed**: {elapsed:.2}s\n\n\
            | Workload | QPS | P50 (ms) | P95 (ms) | P99 (ms) | Max (ms) |\n\
            | :--- | :--- | :--- | :--- | :--- | :--- |\n\
            | OLTP Inserts | {ins_qps:.1} | {ins_p50:.3} | {ins_p95:.3} | {ins_p99:.3} | {ins_max:.3} |\n\
            | OLTP Point Lookups | {look_qps:.1} | {look_p50:.3} | {look_p95:.3} | {look_p99:.3} | {look_max:.3} |\n\
            | Vector HNSW Search | {vec_qps:.1} | {vec_p50:.3} | {vec_p95:.3} | {vec_p99:.3} | {vec_max:.3} |\n\
            | Vectorized OLAP | {olap_qps:.1} | {olap_p50:.3} | {olap_p95:.3} | {olap_p99:.3} | {olap_max:.3} |\n\n\
            ### Storage & Calibrated CBO Constants\n\
            - **Disk Footprint**: {disk:.2} MB (WAF: {waf:.2}x)\n\
            - **SeqScan (1k rows)**: {w_scan:.4} ms\n\
            - **BTree Point Lookup**: {w_btree:.4} ms\n\
            - **Vector Comparison**: {w_vec:.6} ms\n",
            scale = report.scale_label,
            rows = report.total_rows,
            conc = report.concurrency,
            elapsed = report.elapsed_total_secs,
            ins_qps = report.oltp_inserts.qps,
            ins_p50 = report.oltp_inserts.percentiles.p50_ms,
            ins_p95 = report.oltp_inserts.percentiles.p95_ms,
            ins_p99 = report.oltp_inserts.percentiles.p99_ms,
            ins_max = report.oltp_inserts.percentiles.max_ms,
            look_qps = report.oltp_lookups.qps,
            look_p50 = report.oltp_lookups.percentiles.p50_ms,
            look_p95 = report.oltp_lookups.percentiles.p95_ms,
            look_p99 = report.oltp_lookups.percentiles.p99_ms,
            look_max = report.oltp_lookups.percentiles.max_ms,
            vec_qps = report.vector_hnsw.qps,
            vec_p50 = report.vector_hnsw.percentiles.p50_ms,
            vec_p95 = report.vector_hnsw.percentiles.p95_ms,
            vec_p99 = report.vector_hnsw.percentiles.p99_ms,
            vec_max = report.vector_hnsw.percentiles.max_ms,
            olap_qps = report.columnar_olap.qps,
            olap_p50 = report.columnar_olap.percentiles.p50_ms,
            olap_p95 = report.columnar_olap.percentiles.p95_ms,
            olap_p99 = report.columnar_olap.percentiles.p99_ms,
            olap_max = report.columnar_olap.percentiles.max_ms,
            disk = report.storage.total_disk_mb,
            waf = report.storage.estimated_waf,
            w_scan = report.calibrated_weights.cost_per_1k_seq_scan_ms,
            w_btree = report.calibrated_weights.cost_btree_lookup_ms,
            w_vec = report.calibrated_weights.cost_vector_dist_calc_ms,
        );
        std::fs::write(&md_path, md)?;
        println!("✅ Wrote Markdown benchmark report to: {}", md_path);
    }

    let _ = std::fs::remove_dir_all(&bench_dir);
    Ok(())
}
