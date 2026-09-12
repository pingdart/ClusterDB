import requests
from typing import Any, Dict, List, Optional
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from rich.rule import Rule

from chaos_engine.self_learning import SelfLearningEngine

console = Console(safe_box=True)

class RBACAttackEngine:
    def __init__(self, db_client, self_learning: Optional[SelfLearningEngine] = None):
        self.db = db_client
        self.learner = self_learning or SelfLearningEngine()

    def run(self):
        console.print(Rule("[bold red]Layer 3: RBAC & Tenant Attack Engine (Penetration Probes)[/bold red]"))
        console.print("[cyan]Testing authorization boundaries, role escalation, and tenant isolation...[/cyan]\n")

        # Ensure a test target database exists
        self.db.execute_query("CREATE DATABASE rbac_sec_db;")
        self.db.execute_query("CREATE TABLE secrets (id INT PRIMARY KEY, key VARCHAR(50), val VARCHAR(100));", "rbac_sec_db")
        self.db.execute_query("INSERT INTO secrets VALUES (1, 'master_key', 'super_secret_123');", "rbac_sec_db")

        test_cases = [
            {
                "name": "Unauthenticated Query",
                "token": "",
                "query": "SELECT * FROM secrets;",
                "db": "rbac_sec_db",
                "expected_blocked": True,
                "desc": "Request with empty token should be rejected (401)"
            },
            {
                "name": "Forged Session Token",
                "token": "forged-jwt-token-hacker-payload",
                "query": "SELECT * FROM secrets;",
                "db": "rbac_sec_db",
                "expected_blocked": True,
                "desc": "Request with fabricated token should be rejected"
            },
            {
                "name": "Malformed UUID Token",
                "token": "00000000-0000-0000-0000-000000000000",
                "query": "DROP TABLE secrets;",
                "db": "rbac_sec_db",
                "expected_blocked": True,
                "desc": "Random nil UUID token should be blocked"
            },
            {
                "name": "Cross-Tenant Access Probe",
                "token": self.db.token,
                "query": "SELECT * FROM secrets;",
                "db": "rbac_sec_db",
                "tenant": "attacker_tenant_xyz",
                "expected_blocked": True,
                "desc": "Foreign tenant querying secrets isolated to target tenancy"
            }

        ]

        results = []
        leaks = 0

        for tc in test_cases:
            payload = {
                "token": tc["token"],
                "query": tc["query"],
                "database": tc["db"]
            }
            if "tenant" in tc:
                payload["tenant_id"] = tc["tenant"]

            try:
                res = self.db.session.post(f"{self.db.base_url}/query", json=payload, timeout=5)
                status_code = res.status_code
                data = res.json() if res.status_code in [200, 400, 401, 403, 429] else {}
            except Exception as e:
                status_code = 500
                data = {"status": "error", "error": str(e)}

            is_blocked = (status_code in [401, 403]) or (data.get("status") == "error")

            if tc["expected_blocked"]:
                passed = is_blocked
            else:
                passed = (status_code == 200 and data.get("status") == "success")

            if not passed and tc["expected_blocked"]:
                leaks += 1
                self.learner.record_bug(
                    layer="Layer 3: RBAC Attacker",
                    title=f"Security Leak: {tc['name']} was NOT blocked!",
                    query=tc["query"],
                    error=f"Expected rejection, got HTTP {status_code}: {data}",
                    repro_steps=[f"Token: {tc['token']}", f"Query: {tc['query']}"]
                )

            results.append({
                "name": tc["name"],
                "http_status": status_code,
                "blocked": is_blocked,
                "passed": passed
            })

        table = Table(title="[bold white]RBAC & Authorization Audit Matrix[/bold white]", box=None, padding=(0, 2))
        table.add_column("Security Attack Vector", style="bold cyan")
        table.add_column("HTTP Code", justify="center", style="yellow")
        table.add_column("Access Blocked?", justify="center")
        table.add_column("Audit Verdict", justify="center")

        for r in results:
            blocked_badge = "[green]YES (Protected)[/green]" if r["blocked"] else "[red]NO (Leaked)[/red]"
            verdict = "[bold green]PASS[/bold green]" if r["passed"] else "[bold red]FAIL (SECURITY LEAK)[/bold red]"
            table.add_row(r["name"], str(r["http_status"]), blocked_badge, verdict)

        console.print(Panel(table, border_style="red" if leaks > 0 else "green"))

        if leaks == 0:
            console.print("[bold green]✔ Zero security leaks detected. All unauthorized endpoints returned 401/403![/bold green]\n")
        else:
            console.print(f"[bold red]✘ CRITICAL: {leaks} authorization boundary leaks detected![/bold red]\n")
