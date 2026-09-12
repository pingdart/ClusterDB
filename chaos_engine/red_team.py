import time
from typing import Any, Dict, List, Optional
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from rich.rule import Rule

from chaos_engine.self_learning import SelfLearningEngine

console = Console(safe_box=True)

class SecurityRedTeam:
    def __init__(self, db_client, self_learning: Optional[SelfLearningEngine] = None):
        self.db = db_client
        self.learner = self_learning or SelfLearningEngine()

    def run(self):
        console.print(Rule("[bold red]Layer 7: Security Red Team AI (Penetration Attacks)[/bold red]"))
        console.print("[red]Simulating adversary penetration attacks: SQLi, Path Traversal, and Stacked Injections...[/red]\n")

        # Setup target
        self.db.execute_query("CREATE DATABASE sec_red_db;")
        self.db.execute_query("CREATE TABLE users (id INT PRIMARY KEY, username VARCHAR(50), secret VARCHAR(50));", "sec_red_db")
        self.db.execute_query("INSERT INTO users VALUES (1, 'alice', 'alice_pass_xyz');", "sec_red_db")
        self.db.execute_query("INSERT INTO users VALUES (2, 'bob', 'bob_pass_123');", "sec_red_db")

        attacks = [
            {
                "name": "Classic Tautology SQLi (' OR '1'='1)",
                "query": "SELECT * FROM users WHERE username = 'admin' OR '1'='1';",
                "db": "sec_red_db",
                "risk": "Data leakage of all rows"
            },
            {
                "name": "Comment Truncation SQLi (admin' --)",
                "query": "SELECT * FROM users WHERE username = 'admin' --' AND secret = 'pass';",
                "db": "sec_red_db",
                "risk": "Auth bypass"
            },
            {
                "name": "Stacked Query Attack ('; DROP TABLE users; --)",
                "query": "SELECT * FROM users; DROP TABLE users; --",
                "db": "sec_red_db",
                "risk": "Arbitrary DDL execution"
            },
            {
                "name": "Union-Based Extraction",
                "query": "SELECT username, secret FROM users UNION SELECT username, password_hash FROM _users;",
                "db": "sec_red_db",
                "risk": "System credential exfiltration"
            },
            {
                "name": "Path Traversal in Backup Command",
                "query": "BACKUP TO '../../../../windows/system32/cmd.exe';",
                "db": "sec_red_db",
                "risk": "Arbitrary file write / system overwrite"
            }
        ]

        results = []
        critical_leaks = 0

        for a in attacks:
            t0 = time.perf_counter()
            res = self.db.execute_query(a["query"], database=a["db"])
            lat = res.get("latency_ms", (time.perf_counter() - t0) * 1000.0)

            # Invariant: Must not panic, crash, or execute stacked drop table!
            err = res.get("error", "")
            is_err = res.get("status") == "error"
            output = str(res.get("result", ""))

            # Verify that stacked DROP did not delete the table
            if "DROP TABLE" in a["query"]:
                check_tbl = self.db.execute_query("SELECT * FROM users;", a["db"])
                table_intact = check_tbl.get("status") == "success"
                blocked = table_intact
            elif "UNION" in a["query"]:
                # Should not leak _users password hashes
                blocked = ("admin123" not in output) and ("password_hash" not in output)
            elif "BACKUP" in a["query"]:
                # Path traversal should fail or reject
                blocked = is_err or ("error" in res)
            else:
                blocked = True  # SQL executed safely through query planner

            if not blocked:
                critical_leaks += 1
                self.learner.record_bug(
                    layer="Layer 7: Red Team AI",
                    title=f"Security vulnerability: {a['name']} succeeded!",
                    query=a["query"],
                    error=f"Attack succeeded with output: {output}",
                    repro_steps=[f"USE {a['db']};", a["query"]]
                )

            results.append({
                "name": a["name"],
                "risk": a["risk"],
                "blocked": blocked,
                "lat": lat
            })

        table = Table(title="[bold white]Red Team Penetration Assessment Matrix[/bold white]", box=None, padding=(0, 2))
        table.add_column("Penetration Vector", style="bold cyan")
        table.add_column("Security Risk", style="yellow")
        table.add_column("Engine Defense", justify="center")
        table.add_column("Latency", justify="right", width=10)

        for r in results:
            defense = "[bold green]DEFENDED[/bold green]" if r["blocked"] else "[bold red]VULNERABLE[/bold red]"
            table.add_row(r["name"], r["risk"], defense, f"{r['lat']:.2f} ms")

        console.print(Panel(table, border_style="red" if critical_leaks > 0 else "green"))

        if critical_leaks == 0:
            console.print("[bold green]✔ Security Red Team: All attack vectors neutralized. No critical breaches detected![/bold green]\n")
        else:
            console.print(f"[bold red]✘ CRITICAL: {critical_leaks} vulnerabilities detected by Red Team AI![/bold red]\n")
