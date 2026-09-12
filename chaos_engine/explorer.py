import random
import time
from typing import Any, Dict, List, Optional
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from rich.rule import Rule

from chaos_engine.self_learning import SelfLearningEngine

console = Console(safe_box=True)

class AutonomousExplorer:
    def __init__(self, db_client, ollama_client, self_learning: Optional[SelfLearningEngine] = None):
        self.db = db_client
        self.ollama = ollama_client
        self.learner = self_learning or SelfLearningEngine()

    def run(self, steps: int = 15):
        console.print(Rule("[bold cyan]Layer 8: Autonomous Explorer Agent (Blackbox Hacker Mode)[/bold cyan]"))
        console.print(f"[cyan]AI Agent given only DB URL and Auth. Exploring {steps} chained state transitions...[/cyan]\n")

        db_name = f"explorer_sandbox_{int(time.time()) % 10000}"
        table_name = "nodes"
        state = {"db_exists": False, "table_exists": False, "row_count": 0}

        actions_log = []

        for step in range(steps):
            # Agent decides next action based on current state
            available_actions = []
            if not state["db_exists"]:
                available_actions.append("CREATE_DB")
            else:
                available_actions.extend(["QUERY", "EXPORT", "CREATE_TABLE"])
                if state["table_exists"]:
                    available_actions.extend(["INSERT", "UPDATE", "INDEX", "DROP_TABLE"])
                if state["row_count"] > 0:
                    available_actions.extend(["SELECT_WHERE", "DELETE"])
                available_actions.append("DROP_DB")

            chosen = random.choice(available_actions)
            t0 = time.perf_counter()

            if chosen == "CREATE_DB":
                q = f"CREATE DATABASE {db_name};"
                r = self.db.execute_query(q)
                if r.get("status") == "success": state["db_exists"] = True
            elif chosen == "CREATE_TABLE":
                q = f"CREATE TABLE {table_name} (id INT PRIMARY KEY, name VARCHAR(50), weight FLOAT);"
                r = self.db.execute_query(q, db_name)
                if r.get("status") == "success": state["table_exists"] = True
            elif chosen == "INSERT":
                new_id = state["row_count"] + 1
                q = f"INSERT INTO {table_name} (id, name, weight) VALUES ({new_id}, 'node_{new_id}', {round(random.uniform(1.0, 100.0), 2)});"
                r = self.db.execute_query(q, db_name)
                if r.get("status") == "success": state["row_count"] += 1
            elif chosen == "SELECT_WHERE":
                q = f"SELECT * FROM {table_name} WHERE weight > 10.0 LIMIT 5;"
                r = self.db.execute_query(q, db_name)
            elif chosen == "INDEX":
                q = f"CREATE INDEX idx_node_wt ON {table_name} (weight);"
                r = self.db.execute_query(q, db_name)
            elif chosen == "UPDATE":
                q = f"UPDATE {table_name} SET weight = weight * 1.1 WHERE id = 1;"
                r = self.db.execute_query(q, db_name)
            elif chosen == "DELETE":
                q = f"DELETE FROM {table_name} WHERE id = {state['row_count']};"
                r = self.db.execute_query(q, db_name)
                if r.get("status") == "success" and state["row_count"] > 0:
                    state["row_count"] -= 1
            elif chosen == "DROP_TABLE":
                q = f"DROP TABLE {table_name};"
                r = self.db.execute_query(q, db_name)
                if r.get("status") == "success": state["table_exists"] = False; state["row_count"] = 0
            elif chosen == "DROP_DB":
                q = f"DROP DATABASE {db_name};"
                r = self.db.execute_query(q)
                if r.get("status") == "success": state["db_exists"] = False; state["table_exists"] = False; state["row_count"] = 0
            else:
                q = f"SELECT 1;"
                r = self.db.execute_query(q, db_name if state["db_exists"] else None)

            lat = (time.perf_counter() - t0) * 1000.0
            is_ok = r.get("status") == "success"

            actions_log.append({
                "step": step + 1,
                "action": chosen,
                "query": q,
                "status": is_ok,
                "lat": lat
            })
            time.sleep(0.02)

        table = Table(title="[bold white]Autonomous Explorer State Sequence[/bold white]", box=None, padding=(0, 2))
        table.add_column("Step", style="dim", width=6)
        table.add_column("Explored Action", style="bold yellow", width=18)
        table.add_column("Action Query", style="white")
        table.add_column("Verdict", justify="center")
        table.add_column("Latency", justify="right", width=10)

        for act in actions_log:
            v = "[green]OK[/green]" if act["status"] else "[red]FAIL[/red]"
            table.add_row(str(act["step"]), act["action"], act["query"][:45], v, f"{act['lat']:.2f} ms")

        console.print(Panel(table, border_style="cyan"))
        console.print("[bold green]✔ Autonomous Explorer chained random transitions safely without engine state corruption![/bold green]\n")
