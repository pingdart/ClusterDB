import json
import os
import time
from typing import Any, Dict, List, Optional
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from rich.rule import Rule

console = Console(safe_box=True)

PLAN_DIR = os.path.join(".", "ai_learning", "plan_baselines")
BASELINE_FILE = os.path.join(PLAN_DIR, "plan_history.json")

BENCHMARK_WORKLOADS = [
    {
        "name": "PK Point Lookup",
        "query": "SELECT * FROM users WHERE id = 1;",
        "explain_query": "EXPLAIN SELECT * FROM users WHERE id = 1;",
        "expected_plan_hint": "IndexScan"
    },
    {
        "name": "Filtered Range Scan",
        "query": "SELECT * FROM users WHERE status = 'active';",
        "explain_query": "EXPLAIN SELECT * FROM users WHERE status = 'active';",
        "expected_plan_hint": "Filter"
    },
    {
        "name": "Sorted Limit Scan",
        "query": "SELECT * FROM users ORDER BY id DESC LIMIT 10;",
        "explain_query": "EXPLAIN SELECT * FROM users ORDER BY id DESC LIMIT 10;",
        "expected_plan_hint": "Limit"
    },
    {
        "name": "Aggregate Count",
        "query": "SELECT COUNT(*) FROM users;",
        "explain_query": "EXPLAIN SELECT COUNT(*) FROM users;",
        "expected_plan_hint": "Aggregate"
    }
]


class QueryPlanRegressionDetector:
    """
    Phase 14E: Query Plan & Performance Regression Detection Engine.
    Continuously audits EXPLAIN execution plans and latency profiles against
    established baselines, flagging CBO degradations before users notice.
    """

    def __init__(self, db_client):
        self.db = db_client
        os.makedirs(PLAN_DIR, exist_ok=True)
        self.baselines: Dict[str, Dict[str, Any]] = self._load_baselines()

    def _load_baselines(self) -> Dict[str, Dict[str, Any]]:
        if os.path.exists(BASELINE_FILE):
            try:
                with open(BASELINE_FILE, "r", encoding="utf-8") as f:
                    return json.load(f)
            except Exception:
                return {}
        return {}

    def _save_baselines(self):
        try:
            with open(BASELINE_FILE, "w", encoding="utf-8") as f:
                json.dump(self.baselines, f, indent=2)
        except Exception as e:
            console.print(f"[red]Failed to save plan baselines: {e}[/red]")

    def audit_workloads(self) -> List[Dict[str, Any]]:
        results = []
        regressions = 0

        for workload in BENCHMARK_WORKLOADS:
            name = workload["name"]
            query = workload["query"]
            explain_q = workload["explain_query"]

            # 1. Execute EXPLAIN to inspect query planner execution tree
            plan_res = self.db.execute_query(explain_q, "default")
            plan_text = plan_res.get("result", "SeqScan")

            # 2. Measure actual execution latency
            t0 = time.perf_counter()
            exec_res = self.db.execute_query(query, "default")
            measured_lat = exec_res.get("latency_ms", (time.perf_counter() - t0) * 1000.0)

            # 3. Compare with historical baseline
            if name not in self.baselines:
                self.baselines[name] = {
                    "query": query,
                    "baseline_latency_ms": round(measured_lat, 2),
                    "baseline_plan": plan_text,
                    "first_recorded": time.strftime("%Y-%m-%d %H:%M:%SZ"),
                    "last_checked": time.strftime("%Y-%m-%d %H:%M:%SZ")
                }
                self._save_baselines()
                results.append({
                    "name": name,
                    "baseline_lat": measured_lat,
                    "current_lat": measured_lat,
                    "plan": plan_text,
                    "regression": False,
                    "shift": "Baseline Initialized"
                })
                continue

            base = self.baselines[name]
            base_lat = base.get("baseline_latency_ms", 1.0)
            base_plan = base.get("baseline_plan", "")

            base_type = self._extract_plan_type(base_plan)
            curr_type = self._extract_plan_type(plan_text)

            # Latency regression check: > 3x baseline and > 5.0 ms
            is_latency_reg = (measured_lat > base_lat * 3.0) and (measured_lat > 5.0)
            
            # Plan regression check: Plan degenerated from IndexScan to Seq/CollectionScan
            is_plan_reg = base_type.startswith("IndexScan") and not curr_type.startswith("IndexScan")

            has_regression = is_latency_reg or is_plan_reg
            if has_regression:
                regressions += 1

            shift_desc = "Optimal (Matched Baseline)"
            if is_plan_reg:
                shift_desc = f"Plan Shift: {base_type} -> {curr_type}"
            elif is_latency_reg:
                shift_desc = f"Latency Regression: {base_lat:.2f}ms -> {measured_lat:.2f}ms"

            base["last_checked"] = time.strftime("%Y-%m-%d %H:%M:%SZ")
            self._save_baselines()

            results.append({
                "name": name,
                "baseline_lat": base_lat,
                "current_lat": measured_lat,
                "plan": curr_type,
                "regression": has_regression,
                "shift": shift_desc
            })

        return results

    @staticmethod
    def _extract_plan_type(plan_raw: str) -> str:
        try:
            data = json.loads(plan_raw)
            if isinstance(data, dict):
                idx = data.get("index_used")
                if idx and str(idx).lower() not in ["null", "none", ""]:
                    return f"IndexScan({idx})"
                return data.get("execution_type", "CollectionScan")
        except Exception:
            pass
        low = plan_raw.lower()
        if "index_used" in low and '"index_used": null' not in low and '"index_used":null' not in low:
            return "IndexScan"
        if "indexscan" in low:
            return "IndexScan"
        return "CollectionScan"

    def print_plan_audit_report(self):
        console.print(Rule("[bold magenta]PHASE 14E: QUERY PLAN & PERFORMANCE REGRESSION DETECTOR[/bold magenta]"))
        results = self.audit_workloads()

        tbl = Table(box=None, padding=(0, 2))
        tbl.add_column("Query Workload", style="bold cyan")
        tbl.add_column("Baseline Latency", style="dim", justify="right")
        tbl.add_column("Measured Latency", style="bold white", justify="right")
        tbl.add_column("Planner Evaluation", style="white")
        tbl.add_column("Regression Status", justify="center")

        has_any_reg = any(r["regression"] for r in results)

        for r in results:
            status_badge = "[bold red]REGRESSION[/bold red]" if r["regression"] else "[bold green]PASS (NO REGRESSION)[/bold green]"
            tbl.add_row(
                r["name"],
                f"{r['baseline_lat']:.2f} ms",
                f"{r['current_lat']:.2f} ms",
                r["shift"],
                status_badge
            )

        console.print(Panel(tbl, border_style="red" if has_any_reg else "green", title="[bold white]CBO Execution Plan Stability Scorecard[/bold white]"))
