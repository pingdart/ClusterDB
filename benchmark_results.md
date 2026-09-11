# PingDartDB Scale Benchmark Report (10k)

- **Total Rows**: 10000
- **Concurrency**: 8 workers
- **Total Elapsed**: 88.95s

| Workload | QPS | P50 (ms) | P95 (ms) | P99 (ms) | Max (ms) |
| :--- | :--- | :--- | :--- | :--- | :--- |
| OLTP Inserts | 113.3 | 8.934 | 12.987 | 14.926 | 83.443 |
| OLTP Point Lookups | 87602.7 | 0.011 | 0.012 | 0.013 | 0.107 |
| Vector HNSW Search | 873.0 | 0.970 | 1.904 | 3.124 | 3.124 |
| Vectorized OLAP | 317.4 | 3.062 | 5.728 | 5.728 | 5.728 |

### Storage & Calibrated CBO Constants
- **Disk Footprint**: 8.00 MB (WAF: 6.55x)
- **SeqScan (1k rows)**: 9.4511 ms
- **BTree Point Lookup**: 0.0113 ms
- **Vector Comparison**: 0.017899 ms
