import json
from typing import Any, Dict, List, Optional
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from rich.rule import Rule

from chaos_engine.self_learning import SelfLearningEngine

console = Console(safe_box=True)

class ImportExportTorture:
    def __init__(self, db_client, self_learning: Optional[SelfLearningEngine] = None):
        self.db = db_client
        self.learner = self_learning or SelfLearningEngine()

    def run(self):
        console.print(Rule("[bold magenta]Layer 5: Import/Export Torture Tests[/bold magenta]"))
        console.print("[cyan]Testing parser resiliency against malformed JSON, corrupted CSVs, and truncated data...[/cyan]\n")

        # Setup target database
        self.db.execute_query("CREATE DATABASE torture_db;")

        torture_cases = [
            {
                "name": "Malformed JSON Import (Unclosed string)",
                "endpoint": "/backup/import",
                "payload": {"database": "torture_db", "token": self.db.token, "data": {"corrupt": '{"key": "unclosed'}},
                "desc": "Tests JSON deserializer error handling"

            },
            {
                "name": "Deeply Nested Dictionary Import",
                "endpoint": "/backup/import",
                "payload": {"database": "torture_db", "token": self.db.token, "data": {"deep": {"a": {"b": {"c": 123}}}}},
                "desc": "Tests schema validator against non-relational nested JSON"
            },
            {
                "name": "Empty Payload Import",
                "endpoint": "/backup/import",
                "payload": {},
                "desc": "Missing token and database fields"
            },
            {
                "name": "Invalid Database Export",
                "endpoint": "/backup/export",
                "payload": {"database": "non_existent_ghost_db_999", "token": self.db.token},
                "desc": "Export request for non-existent database"
            }
        ]

        results = []
        crashes = 0

        for tc in torture_cases:
            url = f"{self.db.base_url}{tc['endpoint']}"
            try:
                res = self.db.session.post(url, json=tc["payload"], timeout=5)
                code = res.status_code
                try:
                    data = res.json()
                except Exception:
                    data = {"status": "raw", "text": res.text[:50]}
            except Exception as e:
                code = 500
                data = {"status": "error", "error": str(e)}
                crashes += 1
                self.learner.record_bug(
                    layer="Layer 5: Import Torture",
                    title=f"Import/Export crash on {tc['name']}",
                    query=f"POST {tc['endpoint']} with {tc['payload']}",
                    error=str(e),
                    repro_steps=[f"POST {url}"]
                )

            # Invariant: Server should return 400 or error status, but NEVER panic/500 crash
            survived = (code != 500) and self.db.is_running()
            results.append({
                "name": tc["name"],
                "http_code": code,
                "survived": survived,
                "desc": tc["desc"]
            })

        table = Table(title="[bold white]Import/Export Resilience Scorecard[/bold white]", box=None, padding=(0, 2))
        table.add_column("Torture Test Scenario", style="bold cyan")
        table.add_column("HTTP Code", justify="center", style="yellow")
        table.add_column("Process Survived?", justify="center")
        table.add_column("Behavior", style="dim")

        for r in results:
            surv_badge = "[green]YES[/green]" if r["survived"] else "[bold red]CRASH[/bold red]"
            table.add_row(r["name"], str(r["http_code"]), surv_badge, r["desc"])

        console.print(Panel(table, border_style="magenta"))

        if crashes == 0:
            console.print("[bold green]✔ All import/export torture cases handled cleanly with proper HTTP responses![/bold green]\n")
        else:
            console.print(f"[bold red]✘ {crashes} import/export crashes detected![/bold red]\n")
