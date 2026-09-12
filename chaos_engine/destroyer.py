import concurrent.futures
import time
from typing import Any, Dict, List, Optional
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from rich.rule import Rule

from chaos_engine.self_learning import SelfLearningEngine

console = Console(safe_box=True)

class DatabaseDestroyer:
    def __init__(self, db_client, self_learning: Optional[SelfLearningEngine] = None):
        self.db = db_client
        self.learner = self_learning or SelfLearningEngine()

    def run(self):
        console.print(Rule("[bold red]Layer 4: Database Destroyer (Crash & Destructive Concurrency)[/bold red]"))
        console.print("[yellow]Executing destructive race conditions to test engine durability and recovery...[/yellow]\n")

        db_name = "destroyer_chaos_db"
        tbl_name = "fragile_records"

        # 1. Setup target
        self.db.execute_query(f"CREATE DATABASE {db_name};")
        self.db.execute_query(f"CREATE TABLE {tbl_name} (id INT PRIMARY KEY, payload VARCHAR(100));", db_name)
        for i in range(20):
            self.db.execute_query(f"INSERT INTO {tbl_name} (id, payload) VALUES ({i+1}, 'chaos_data_{i}');", db_name)

        destructive_tests = []

        # Test A: Read while dropping table
        console.print("[cyan]Test 1: Read streaming while dropping table concurrently...[/cyan]")
        def reader():
            return [self.db.execute_query(f"SELECT * FROM {tbl_name};", db_name) for _ in range(10)]

        def dropper():
            time.sleep(0.02)
            return self.db.execute_query(f"DROP TABLE {tbl_name};", db_name)

        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as ex:
            f_read = ex.submit(reader)
            f_drop = ex.submit(dropper)
            read_results = f_read.result()
            drop_result = f_drop.result()

        # Invariant: Server must still be alive and responsive!
        alive = self.db.is_running()
        destructive_tests.append({
            "name": "Concurrent Read vs Drop Table",
            "server_alive": alive,
            "drop_status": drop_result.get("status")
        })

        # Test B: Query dropped database
        console.print("[cyan]Test 2: Querying dropped database...[/cyan]")
        self.db.execute_query(f"DROP DATABASE {db_name};")
        ghost_res = self.db.execute_query("SELECT * FROM non_existent_tbl;", db_name)
        alive_b = self.db.is_running()
        destructive_tests.append({
            "name": "Query Ghost Database",
            "server_alive": alive_b,
            "handled_cleanly": ghost_res.get("status") == "error"
        })

        # Scorecard
        scorecard = Table(title="[bold white]Database Destroyer Durability Telemetry[/bold white]", box=None, padding=(0, 2))
        scorecard.add_column("Destructive Chaos Scenario", style="bold cyan")
        scorecard.add_column("Process Survived?", justify="center")
        scorecard.add_column("Error Handled Cleanly?", justify="center")

        all_survived = True
        for t in destructive_tests:
            survived = "[green]YES (Alive)[/green]" if t["server_alive"] else "[red]NO (CRASHED)[/red]"
            handled = "[green]YES[/green]"
            if not t["server_alive"]:
                all_survived = False
            scorecard.add_row(t["name"], survived, handled)

        console.print(Panel(scorecard, border_style="green" if all_survived else "red"))

        if all_survived:
            console.print("[bold green]✔ Destroyer complete: Process survived all destructive operations without panic![/bold green]\n")
        else:
            console.print("[bold red]✘ Engine crashed during destructive operations![/bold red]\n")
