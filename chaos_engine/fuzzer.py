import random
import string
import time
from typing import Any, Dict, List, Optional
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from rich.live import Live
from rich.rule import Rule

from chaos_engine.self_learning import SelfLearningEngine

console = Console(safe_box=True)

class SQLFuzzer:
    def __init__(self, db_client, ollama_client, self_learning: Optional[SelfLearningEngine] = None):
        self.db = db_client
        self.ollama = ollama_client
        self.learner = self_learning or SelfLearningEngine()

    def generate_malformed_queries(self) -> List[str]:
        return [
            "SELECT FROM users;",
            "SELECT * users;",
            "INSERT INTO;",
            "DROP TABLE;",
            ";;;;",
            "SELECT * FROM users WHERE;",
            "UPDATE users SET;",
            "SELECT * FROM users WHERE id = ;",
            "SELECT * FROM WHERE id = 1;",
            "INSERT INTO users VALUES ();",
            "CREATE TABLE (id INT);",
            "CREATE DATABASE ;",
            "SELECT 'unclosed string from users;",
            "SELECT * FROM users ORDER BY ;",
            "SELECT * FROM users LIMIT -10;",
            "SELECT * FROM users OFFSET -5;",
            "DELETE FROM ;",
            "UPDATE SET name = 'test';",
            "SELECT * FROM non_existent_table_9999999;",
            "SELECT * FROM users WHERE id IN ();",
            "SELECT * FROM users JOIN ;",
            "SELECT " + "A" * 5000 + " FROM users;",
            "SELECT * FROM users WHERE name = '" + "X" * 10000 + "';",
            "SELECT * FROM users WHERE id = 999999999999999999999999999999999999999;",
            "SELECT * FROM users WHERE id = -9223372036854775809;",
            "SELECT * FROM users WHERE age = NaN;",
            "SELECT * FROM users WHERE age = Infinity;",
            "SELECT '\x00\x01\x02\x03' FROM users;",
            "SELECT '🔥🚀💻🎉' FROM users WHERE name = '🔥';",
        ]

    def generate_complex_valid_queries(self, table: str = "users") -> List[str]:
        return [
            f"SELECT * FROM {table};",
            f"SELECT * FROM {table} WHERE age > 20;",
            f"SELECT * FROM {table} WHERE age > 20 AND name = 'Alice';",
            f"SELECT * FROM {table} ORDER BY age DESC LIMIT 10;",
            f"SELECT id, name FROM {table} WHERE age >= 18 AND age <= 65 LIMIT 5 OFFSET 2;",
            f"SELECT * FROM {table} WHERE status = 'active' ORDER BY id ASC;",
            f"UPDATE {table} SET age = 30 WHERE id = 1;",
            f"DELETE FROM {table} WHERE id = 9999;",
            f"SELECT * FROM {table} WHERE name != 'Ghost' LIMIT 20;",
            f"SELECT * FROM {table} WHERE id = 1 OR id = 2 OR id = 3;",
        ]

    def run(self, count: int = 200, target_db: str = "fuzz_db"):
        console.print(Rule("[bold magenta]Layer 1: AI SQL Fuzzer & Boundary Torture[/bold magenta]"))
        console.print(f"[cyan]Targeting Database:[/cyan] {target_db} | [cyan]Total Probes:[/cyan] {count:,}\n")

        # Setup dedicated fuzz database & table
        self.db.execute_query(f"CREATE DATABASE {target_db};")
        self.db.execute_query(f"CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(100), age INT, status VARCHAR(20));", target_db)
        self.db.execute_query(f"INSERT INTO users (id, name, age, status) VALUES (1, 'Alice', 25, 'active');", target_db)
        self.db.execute_query(f"INSERT INTO users (id, name, age, status) VALUES (2, 'Bob', 30, 'pending');", target_db)

        malformed = self.generate_malformed_queries()
        valid = self.generate_complex_valid_queries()

        crashes = 0
        panics = 0
        timeouts = 0
        clean_rejections = 0
        valid_successes = 0
        current_op = 0

        recent_fuzz: List[Dict[str, Any]] = []

        start_time = time.time()

        def make_fuzz_panel():
            t = Table(box=None, expand=True, padding=(0, 2))
            t.add_column("Probes Executed", style="bold cyan")
            t.add_column("Clean Rejections", style="bold green")
            t.add_column("Valid Executions", style="bold blue")
            t.add_column("Crashes / Panics", style="bold red")

            t.add_row(
                f"{current_op:,} / {count:,}",
                f"{clean_rejections:,}",
                f"{valid_successes:,}",
                f"[red]{crashes + panics}[/red]" if (crashes + panics) > 0 else "[green]0[/green]"
            )

            # Activity table
            act = Table(title="[bold white]Live Fuzzing Stream[/bold white]", box=None, padding=(0, 1), expand=True)
            act.add_column("Query Snippet", width=45)
            act.add_column("Verdict", width=18)
            act.add_column("Latency", justify="right", width=10)

            for item in reversed(recent_fuzz[-5:]):
                act.add_row(item["query"], item["verdict"], f"{item['lat']:.2f} ms")

            grid = Table.grid(padding=1)
            grid.add_row(Panel(t, title="[bold white]SQL Fuzzing Invariant Telemetry[/bold white]", border_style="magenta"))
            grid.add_row(Panel(act, border_style="cyan"))
            return grid

        with Live(make_fuzz_panel(), refresh_per_second=4, console=console) as live:
            for i in range(count):
                current_op = i + 1
                # 70% malformed boundary chaos, 30% complex queries

                if random.random() < 0.70:
                    q = random.choice(malformed)
                    is_expected_fail = True
                else:
                    q = random.choice(valid)
                    is_expected_fail = False

                t0 = time.perf_counter()
                try:
                    res = self.db.execute_query(q, target_db)
                    lat = res.get("latency_ms", (time.perf_counter() - t0) * 1000.0)
                except Exception as e:
                    # Possible crash!
                    lat = (time.perf_counter() - t0) * 1000.0
                    crashes += 1
                    bug_id = self.learner.record_bug(
                        layer="Layer 1: SQL Fuzzer",
                        title=f"Database connection error during fuzzing: {str(e)[:50]}",
                        query=q,
                        error=str(e),
                        repro_steps=[f"USE {target_db};", q]
                    )
                    res = {"status": "error", "error": f"CRASH: {e}", "latency_ms": lat}

                err = str(res.get("error", ""))
                is_success = res.get("status") == "success"

                if "panic" in err.lower() or "internal error" in err.lower():
                    panics += 1
                    self.learner.record_bug(
                        layer="Layer 1: SQL Fuzzer",
                        title="Server panic detected on malformed SQL",
                        query=q,
                        error=err,
                        repro_steps=[f"USE {target_db};", q]
                    )
                    verdict = "[bold red][PANIC][/bold red]"
                elif lat > 500.0:
                    timeouts += 1
                    verdict = "[bold yellow][TIMEOUT][/bold yellow]"
                elif is_success:
                    valid_successes += 1
                    verdict = "[bold green][SUCCESS][/bold green]"
                else:
                    clean_rejections += 1
                    verdict = "[bold green][CLEAN REJECT][/bold green]"

                q_disp = q[:42] + "..." if len(q) > 42 else q
                recent_fuzz.append({"query": q_disp, "verdict": verdict, "lat": lat})
                if len(recent_fuzz) > 10:
                    recent_fuzz.pop(0)

                live.update(make_fuzz_panel())
                time.sleep(0.01)

        # Summary
        console.print()
        scorecard = Table(title="[bold white]Layer 1 Fuzzing Results[/bold white]", box=None, padding=(0, 2))
        scorecard.add_column("Fuzzing Invariant", style="bold cyan")
        scorecard.add_column("Status", style="bold white")
        scorecard.add_row("Total Fuzz Queries Sent", f"{count:,}")
        scorecard.add_row("Clean Error Rejections", f"[green]{clean_rejections:,}[/green]")
        scorecard.add_row("Valid Query Executions", f"[green]{valid_successes:,}[/green]")
        scorecard.add_row("Engine Panics", f"[red]{panics}[/red]" if panics > 0 else "[green]0[/green]")
        scorecard.add_row("Database Crashes", f"[red]{crashes}[/red]" if crashes > 0 else "[green]0[/green]")
        scorecard.add_row("Query Timeouts (>500ms)", f"[yellow]{timeouts}[/yellow]" if timeouts > 0 else "[green]0[/green]")

        status_color = "green" if (crashes == 0 and panics == 0) else "red"
        grade = "PASS (10/10)" if (crashes == 0 and panics == 0) else "FAIL"
        scorecard.add_row("Fuzz Invariant Verdict", f"[bold {status_color}]{grade}[/bold {status_color}]")
        console.print(Panel(scorecard, border_style=status_color))
        console.print()
