import json
import os
import time
import requests
from typing import Any, Dict, List, Optional
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from rich.rule import Rule

console = Console(safe_box=True)

COVERAGE_DIR = os.path.join(".", "ai_learning", "coverage")
COVERAGE_FILE = os.path.join(COVERAGE_DIR, "coverage_matrix.json")

# Catalog of all 42 endpoints in PingDart Core Engine (src/server.rs)
ENDPOINT_CATALOG = [
    {"endpoint": "/", "method": "GET", "category": "System", "desc": "Engine Capabilities & Status"},
    {"endpoint": "/login", "method": "POST", "category": "Auth", "desc": "Admin & User Session Login"},
    {"endpoint": "/query", "method": "POST", "category": "Core SQL", "desc": "Relational SQL Query Execution"},
    {"endpoint": "/document/query", "method": "POST", "category": "Document", "desc": "NoSQL Document Filter Query"},
    {"endpoint": "/database/export", "method": "POST", "category": "Data", "desc": "JSON Database Export"},
    {"endpoint": "/database/import", "method": "POST", "category": "Data", "desc": "JSON Database Import"},
    {"endpoint": "/index/create", "method": "POST", "category": "Index", "desc": "Secondary BTree/Bitmap Index Creation"},
    {"endpoint": "/index/drop", "method": "POST", "category": "Index", "desc": "Secondary Index Drop"},
    {"endpoint": "/index/list", "method": "POST", "category": "Index", "desc": "Catalog Secondary Indexes"},
    {"endpoint": "/index/stats", "method": "POST", "category": "Index", "desc": "Index Depth & Cardinality Stats"},
    {"endpoint": "/vector/search", "method": "POST", "category": "Vector", "desc": "HNSW Cosine Vector Search"},
    {"endpoint": "/search/text", "method": "POST", "category": "Search", "desc": "BM25 Full-Text Search"},
    {"endpoint": "/search/hybrid", "method": "POST", "category": "Search", "desc": "Reciprocal Rank Hybrid Search"},
    {"endpoint": "/ai/memory/store", "method": "POST", "category": "Cognitive AI", "desc": "Episodic & Working Memory Store"},
    {"endpoint": "/ai/memory/recall", "method": "POST", "category": "Cognitive AI", "desc": "Semantic Memory Recall"},
    {"endpoint": "/ai/memory/consolidate", "method": "POST", "category": "Cognitive AI", "desc": "Sleep-Cycle Memory Consolidation"},
    {"endpoint": "/graph/node", "method": "POST", "category": "Graph", "desc": "Labeled Property Graph Node Create"},
    {"endpoint": "/graph/edge", "method": "POST", "category": "Graph", "desc": "Directed Graph Edge Relationship"},
    {"endpoint": "/graph/path", "method": "POST", "category": "Graph", "desc": "Shortest Path BFS Graph Traversal"},
    {"endpoint": "/graph/neighbors", "method": "POST", "category": "Graph", "desc": "1-Hop & N-Hop Graph Neighbor Query"},
    {"endpoint": "/cdc/replay", "method": "POST", "category": "CDC", "desc": "Durable CDC Commit Log Stream"},
    {"endpoint": "/cluster/status", "method": "GET", "category": "Cluster", "desc": "Raft Cluster Topology & Leader Info"},
    {"endpoint": "/cluster/join", "method": "POST", "category": "Cluster", "desc": "Join Cluster Seed Node"},
    {"endpoint": "/cluster/shard/vector_search", "method": "POST", "category": "Cluster", "desc": "Distributed Scatter-Gather Vector Search"},
    {"endpoint": "/raft/request_vote", "method": "POST", "category": "Raft", "desc": "Raft Consensus Election Vote"},
    {"endpoint": "/raft/append_entries", "method": "POST", "category": "Raft", "desc": "Raft Log Replication Heartbeat"},
    {"endpoint": "/tenant/create", "method": "POST", "category": "Multi-Tenant", "desc": "Tenant Isolation & Quota Provisioning"},
    {"endpoint": "/tenant/list", "method": "POST", "category": "Multi-Tenant", "desc": "Active Tenants List"},
    {"endpoint": "/tenant/policy", "method": "POST", "category": "Multi-Tenant", "desc": "Tenant Storage & Compute Quota"},
    {"endpoint": "/tenant/rls", "method": "POST", "category": "Multi-Tenant", "desc": "Row-Level Security Tenant Filter"},
    {"endpoint": "/metrics", "method": "GET", "category": "Observability", "desc": "Prometheus Metrics Endpoint"},
    {"endpoint": "/health/live", "method": "GET", "category": "Observability", "desc": "K8s Liveness Probe"},
    {"endpoint": "/health/ready", "method": "GET", "category": "Observability", "desc": "K8s Readiness Probe"},
    {"endpoint": "/observability/traces", "method": "GET", "category": "Observability", "desc": "OpenTelemetry Trace Spans List"},
    {"endpoint": "/observability/slow_queries", "method": "GET", "category": "Observability", "desc": "Slow Query Log Telemetry"},
    {"endpoint": "/backup/snapshot", "method": "POST", "category": "Backup", "desc": "Point-in-Time Snapshot Creation"},
    {"endpoint": "/backup/restore", "method": "POST", "category": "Backup", "desc": "Snapshot Disaster Recovery Restore"},
    {"endpoint": "/backup/pitr", "method": "POST", "category": "Backup", "desc": "CDC-Replayed Point-in-Time Recovery"},
    {"endpoint": "/backup/export", "method": "POST", "category": "Backup", "desc": "Protobuf ZSTD Encrypted Export"},
    {"endpoint": "/backup/import", "method": "POST", "category": "Backup", "desc": "Protobuf ZSTD Encrypted Import"},
    {"endpoint": "/security/apikey/create", "method": "POST", "category": "Security", "desc": "Scoped Live API Key Generation"},
    {"endpoint": "/security/role/list", "method": "GET", "category": "Security", "desc": "Granular RBAC Role Definitions"}
]


class CoverageIntelligenceEngine:
    """
    Phase 14C: Coverage Intelligence Engine.
    Maps all 42 PingDart endpoints, tracks unique call paths, detects blind spots,
    and actively synthesizes probe payloads to exercise untested database subsystems.
    """

    def __init__(self, db_client):
        self.db = db_client
        os.makedirs(COVERAGE_DIR, exist_ok=True)
        self.matrix: Dict[str, Dict[str, Any]] = self._load_or_init_matrix()

    def _load_or_init_matrix(self) -> Dict[str, Dict[str, Any]]:
        if os.path.exists(COVERAGE_FILE):
            try:
                with open(COVERAGE_FILE, "r", encoding="utf-8") as f:
                    return json.load(f)
            except Exception:
                pass

        matrix = {}
        for ep in ENDPOINT_CATALOG:
            key = f"{ep['method']} {ep['endpoint']}"
            matrix[key] = {
                "endpoint": ep["endpoint"],
                "method": ep["method"],
                "category": ep["category"],
                "description": ep["desc"],
                "calls": 0,
                "status_codes": {},
                "last_tested": None
            }
        return matrix

    def _save_matrix(self):
        try:
            with open(COVERAGE_FILE, "w", encoding="utf-8") as f:
                json.dump(self.matrix, f, indent=2)
        except Exception as e:
            console.print(f"[red]Failed to save coverage matrix: {e}[/red]")

    def record_hit(self, method: str, endpoint: str, status_code: int):
        key = f"{method.upper()} {endpoint}"
        if key not in self.matrix:
            self.matrix[key] = {
                "endpoint": endpoint,
                "method": method.upper(),
                "category": "Dynamic",
                "description": "Dynamic Probe",
                "calls": 0,
                "status_codes": {},
                "last_tested": None
            }
        self.matrix[key]["calls"] += 1
        sc = str(status_code)
        self.matrix[key]["status_codes"][sc] = self.matrix[key]["status_codes"].get(sc, 0) + 1
        self.matrix[key]["last_tested"] = time.strftime("%Y-%m-%d %H:%M:%SZ")
        self._save_matrix()

    def get_stats(self) -> Dict[str, Any]:
        total = len(self.matrix)
        tested = sum(1 for ep in self.matrix.values() if ep["calls"] > 0)
        untested = total - tested
        cov_pct = (tested / total * 100.0) if total > 0 else 0.0
        total_calls = sum(ep["calls"] for ep in self.matrix.values())

        return {
            "total_endpoints": total,
            "tested_endpoints": tested,
            "untested_endpoints": untested,
            "coverage_pct": round(cov_pct, 1),
            "total_calls": total_calls
        }

    def hunt_blindspots(self) -> List[Dict[str, Any]]:
        """
        Actively hunt and execute payloads targeting untested endpoints (0 calls).
        """
        untested = [ep for ep in self.matrix.values() if ep["calls"] == 0]
        if not untested:
            console.print("[bold green]✔ 100% Endpoint Coverage Achieved! Zero blind spots remaining.[/bold green]")
            return []

        console.print(f"[bold yellow]Coverage Hunter: Identified {len(untested)} untested endpoints. Synthesizing probe payloads...[/bold yellow]\n")

        results = []
        token = self.db.token

        for ep in untested[:10]:
            method = ep["method"]
            endpoint = ep["endpoint"]
            url = f"{self.db.base_url}{endpoint}"
            t0 = time.perf_counter()

            try:
                if method == "GET":
                    res = self.db.session.get(url, timeout=5)
                else:
                    # Synthesize targeted JSON payload
                    payload = {"token": token}
                    if "memory" in endpoint:
                        payload.update({"session_id": "hunt_sess_1", "memory_type": "Working", "content": "Autonomous coverage test memory"})
                    elif "graph" in endpoint:
                        payload.update({"id": "node_h1", "labels": ["TestNode"], "properties": {"coverage": True}})
                    elif "vector" in endpoint:
                        payload.update({"database": "default", "table": "users", "vector": [0.1] * 128, "top_k": 5})
                    elif "search" in endpoint:
                        payload.update({"database": "default", "table": "users", "query": "test"})
                    elif "backup" in endpoint:
                        payload.update({"database": "default", "name": "hunt_snapshot"})
                    elif "tenant" in endpoint:
                        payload.update({"tenant_id": "hunt_tenant_1", "name": "Hunt Tenant"})
                    elif "index" in endpoint:
                        payload.update({"database": "default", "table": "users", "column": "id", "index_type": "BTree"})

                    res = self.db.session.post(url, json=payload, timeout=5)

                lat = (time.perf_counter() - t0) * 1000.0
                self.record_hit(method, endpoint, res.status_code)
                results.append({
                    "endpoint": endpoint,
                    "method": method,
                    "status_code": res.status_code,
                    "latency_ms": round(lat, 2)
                })

            except Exception as e:
                self.record_hit(method, endpoint, 500)
                results.append({
                    "endpoint": endpoint,
                    "method": method,
                    "status_code": 500,
                    "latency_ms": 0.0
                })

        return results

    def print_coverage_matrix(self):
        stats = self.get_stats()
        console.print(Rule("[bold cyan]PHASE 14C: TEST COVERAGE INTELLIGENCE MATRIX[/bold cyan]"))

        summary_table = Table(box=None, padding=(0, 2))
        summary_table.add_column("Cataloged Endpoints", style="bold white")
        summary_table.add_column("Tested Paths", style="bold green")
        summary_table.add_column("Untested Blindspots", style="bold red")
        summary_table.add_column("Code Coverage", style="bold magenta")
        summary_table.add_column("Total Test Calls", style="bold cyan")

        cov_color = "green" if stats["coverage_pct"] >= 80 else ("yellow" if stats["coverage_pct"] >= 50 else "red")
        summary_table.add_row(
            f"{stats['total_endpoints']}",
            f"{stats['tested_endpoints']}",
            f"{stats['untested_endpoints']}",
            f"[{cov_color}]{stats['coverage_pct']}%[/{cov_color}]",
            f"{stats['total_calls']:,}"
        )
        console.print(Panel(summary_table, border_style="cyan", title="[bold white]PingDart API Surface Coverage[/bold white]"))

        # Category breakdown
        cat_table = Table(title="[bold white]Subsystem Endpoint Breakdown[/bold white]", box=None, padding=(0, 1))
        cat_table.add_column("Subsystem Category", style="bold cyan", width=22)
        cat_table.add_column("Endpoints", style="white", width=12)
        cat_table.add_column("Tested", style="green", width=10)
        cat_table.add_column("Coverage", justify="right", width=12)

        categories: Dict[str, Dict[str, int]] = {}
        for ep in self.matrix.values():
            cat = ep.get("category", "Other")
            if cat not in categories:
                categories[cat] = {"total": 0, "tested": 0}
            categories[cat]["total"] += 1
            if ep["calls"] > 0:
                categories[cat]["tested"] += 1

        for cat, d in sorted(categories.items()):
            pct = (d["tested"] / d["total"]) * 100.0 if d["total"] > 0 else 0
            color = "green" if pct >= 80 else ("yellow" if pct >= 50 else "red")
            cat_table.add_row(cat, str(d["total"]), str(d["tested"]), f"[{color}]{pct:.1f}%[/{color}]")

        console.print(Panel(cat_table, border_style="magenta"))
