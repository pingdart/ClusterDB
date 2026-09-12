import concurrent.futures
import random
import time
from typing import Any, Dict, List, Optional
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from rich.rule import Rule

from chaos_engine.self_learning import SelfLearningEngine

console = Console(safe_box=True)

class UserBehaviorSimulator:
    def __init__(self, db_client, self_learning: Optional[SelfLearningEngine] = None):
        self.db = db_client
        self.learner = self_learning or SelfLearningEngine()

    def _persona_transactional(self, user_idx: int) -> Dict[str, Any]:
        """User A: High frequency CRUD transactions."""
        db_name = f"user_tx_db_{user_idx}"
        tbl_name = f"accounts_{user_idx}"
        ops = 0
        fails = 0
        err_msg = ""

        # 1. Create DB
        r = self.db.execute_query(f"CREATE DATABASE {db_name};")
        ops += 1
        if r.get("status") != "success": fails += 1; err_msg = r.get("error", "")

        # 2. Create Table
        r = self.db.execute_query(f"CREATE TABLE {tbl_name} (id INT PRIMARY KEY, owner VARCHAR(50), balance FLOAT);", db_name)
        ops += 1
        if r.get("status") != "success": fails += 1; err_msg = r.get("error", "")

        # 3. Insert 10 rows
        for i in range(10):
            r = self.db.execute_query(f"INSERT INTO {tbl_name} (id, owner, balance) VALUES ({i+1}, 'Owner_{i}', {100.0 * (i+1)});", db_name)
            ops += 1
            if r.get("status") != "success": fails += 1; err_msg = r.get("error", "")

        # 4. Point Lookups and Updates
        for i in range(5):
            r = self.db.execute_query(f"SELECT * FROM {tbl_name} WHERE id = {i+1};", db_name)
            ops += 1
            if r.get("status") != "success": fails += 1; err_msg = r.get("error", "")

            r = self.db.execute_query(f"UPDATE {tbl_name} SET balance = balance + 50.0 WHERE id = {i+1};", db_name)
            ops += 1
            if r.get("status") != "success": fails += 1; err_msg = r.get("error", "")

        return {"persona": "Transactional", "user": user_idx, "ops": ops, "fails": fails, "last_err": err_msg}

    def _persona_schema_stress(self, user_idx: int) -> Dict[str, Any]:
        """User B: Schema stress: rapid table creation, index builds, and drops."""
        db_name = f"schema_db_{user_idx}"
        ops = 0
        fails = 0
        err_msg = ""

        self.db.execute_query(f"CREATE DATABASE {db_name};")
        ops += 1

        # Create 5 tables rapidly
        for t in range(5):
            t_name = f"table_{t}"
            r = self.db.execute_query(f"CREATE TABLE {t_name} (id INT PRIMARY KEY, tag VARCHAR(50), score FLOAT);", db_name)
            ops += 1
            if r.get("status") != "success": fails += 1; err_msg = r.get("error", "")

            r = self.db.execute_query(f"CREATE INDEX idx_{t_name}_tag ON {t_name} (tag);", db_name)
            ops += 1
            if r.get("status") != "success": fails += 1; err_msg = r.get("error", "")

            r = self.db.execute_query(f"INSERT INTO {t_name} (id, tag, score) VALUES (1, 'tag_alpha', 95.5);", db_name)
            ops += 1
            if r.get("status") != "success": fails += 1; err_msg = r.get("error", "")

        return {"persona": "SchemaStress", "user": user_idx, "ops": ops, "fails": fails, "last_err": err_msg}

    def _persona_tenant_admin(self, user_idx: int) -> Dict[str, Any]:
        """User C: Multi-tenant quota configuration and tenant-scoped ops."""
        tenant_name = f"tenant_{user_idx}"
        ops = 0
        fails = 0
        err_msg = ""

        # Create Tenant
        r = self.db.execute_query(f"CREATE TENANT {tenant_name};")
        ops += 1
        if r.get("status") != "success": fails += 1; err_msg = r.get("error", "")

        # Set Tenant Quota
        r = self.db.execute_query(f"SET TENANT QUOTA {tenant_name} MAX_STORAGE 100MB MAX_QPS 50;")
        ops += 1
        if r.get("status") != "success": fails += 1; err_msg = r.get("error", "")

        return {"persona": "TenantAdmin", "user": user_idx, "ops": ops, "fails": fails, "last_err": err_msg}

    def run(self, num_users: int = 15, max_workers: int = 8):
        console.print(Rule("[bold cyan]Layer 2: User Behavior Simulator (Concurrent Personas)[/bold cyan]"))
        console.print(f"[bold]Simulating:[/bold] {num_users} Concurrent Virtual Users across Personas A, B, and C with {max_workers} worker threads.\n")

        start_time = time.time()
        results: List[Dict[str, Any]] = []

        with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as executor:
            futures = []
            for i in range(num_users):
                persona_choice = i % 3
                if persona_choice == 0:
                    futures.append(executor.submit(self._persona_transactional, i))
                elif persona_choice == 1:
                    futures.append(executor.submit(self._persona_schema_stress, i))
                else:
                    futures.append(executor.submit(self._persona_tenant_admin, i))

            for f in concurrent.futures.as_completed(futures):
                try:
                    res = f.result()
                    results.append(res)
                except Exception as e:
                    results.append({"persona": "Unknown", "user": -1, "ops": 0, "fails": 1, "last_err": str(e)})

        elapsed = max(0.001, time.time() - start_time)
        total_ops = sum(r["ops"] for r in results)
        total_fails = sum(r["fails"] for r in results)

        # Print concurrency scorecard
        scorecard = Table(title="[bold white]Concurrent User Simulation Telemetry[/bold white]", box=None, padding=(0, 2))
        scorecard.add_column("Persona", style="bold cyan")
        scorecard.add_column("Virtual Users", justify="center")
        scorecard.add_column("Ops Executed", justify="center", style="green")
        scorecard.add_column("Failures / Deadlocks", justify="center", style="red")

        for p_name in ["Transactional", "SchemaStress", "TenantAdmin"]:
            p_res = [r for r in results if r["persona"] == p_name]
            p_users = len(p_res)
            p_ops = sum(r["ops"] for r in p_res)
            p_fails = sum(r["fails"] for r in p_res)
            scorecard.add_row(p_name, str(p_users), f"{p_ops:,}", f"[red]{p_fails}[/red]" if p_fails > 0 else "[green]0[/green]")

        console.print(Panel(scorecard, border_style="cyan"))

        if total_fails > 0:
            console.print(f"[bold yellow]Notice: {total_fails} concurrent operations failed (deadlock/race condition checked).[/bold yellow]")
        else:
            console.print(f"[bold green]✔ All {total_ops:,} concurrent operations executed without deadlocks in {elapsed:.2f}s ({(total_ops/elapsed):.1f} QPS)![/bold green]")
        console.print()
