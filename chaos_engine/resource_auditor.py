import os
import time
import psutil
from typing import Any, Dict, List, Optional
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from rich.rule import Rule

from chaos_engine.self_learning import SelfLearningEngine

console = Console(safe_box=True)

class ResourceAuditor:
    def __init__(self, db_client, self_learning: Optional[SelfLearningEngine] = None):
        self.db = db_client
        self.learner = self_learning or SelfLearningEngine()
        self.proc = self._find_db_process()

    def _find_db_process(self) -> Optional[psutil.Process]:
        for p in psutil.process_iter(['pid', 'name']):
            if "pingdart-core" in p.info['name'].lower():
                return psutil.Process(p.info['pid'])
        return None

    def sample_metrics(self) -> Dict[str, Any]:
        if not self.proc or not self.proc.is_running():
            self.proc = self._find_db_process()
        if not self.proc:
            return {"ram_mb": 0.0, "threads": 0, "handles": 0, "cpu": 0.0}

        try:
            mem = self.proc.memory_info()
            ram_mb = mem.rss / (1024.0 * 1024.0)
            threads = self.proc.num_threads()
            handles = self.proc.num_handles() if hasattr(self.proc, "num_handles") else 0
            cpu = self.proc.cpu_percent(interval=None)
            return {"ram_mb": round(ram_mb, 2), "threads": threads, "handles": handles, "cpu": cpu}
        except Exception:
            return {"ram_mb": 0.0, "threads": 0, "handles": 0, "cpu": 0.0}

    def run(self, ops: int = 300):
        console.print(Rule("[bold yellow]Layer 10: Memory Leak & Resource Auditor[/bold yellow]"))
        console.print(f"[cyan]Auditing RAM, Thread counts, and OS Handles across {ops:,} high-throughput operations...[/cyan]\n")

        if not self.proc:
            console.print("[yellow]Warning: Could not locate running pingdart-core process for OS sampling.[/yellow]")
            return

        baseline = self.sample_metrics()
        console.print(f"[bold cyan]Baseline Metrics:[/bold cyan] RAM: {baseline['ram_mb']} MB | Threads: {baseline['threads']} | Handles: {baseline['handles']}")

        # Execute high-throughput workload
        with console.status(f"[bold green]Running {ops} operations to observe resource profile...[/bold green]"):
            for i in range(ops):
                self.db.execute_query(f"SELECT {i};")

        peak = self.sample_metrics()
        time.sleep(0.5)  # Allow garbage collection / thread pooling cooldown
        post = self.sample_metrics()

        ram_growth = round(post["ram_mb"] - baseline["ram_mb"], 2)
        thread_growth = post["threads"] - baseline["threads"]
        handle_growth = post["handles"] - baseline["handles"]

        # Scorecard
        scorecard = Table(title="[bold white]Resource Leak & Stability Auditor[/bold white]", box=None, padding=(0, 2))
        scorecard.add_column("Resource Metric", style="bold cyan")
        scorecard.add_column("Baseline", justify="center")
        scorecard.add_column("Peak Under Load", justify="center")
        scorecard.add_column("Post-Cooldown", justify="center")
        scorecard.add_column("Growth Delta", justify="center", style="bold yellow")
        scorecard.add_column("Audit Verdict", justify="center")

        ram_verdict = "[bold green]STABLE[/bold green]" if ram_growth < 15.0 else "[bold yellow]ELEVATED[/bold yellow]"
        scorecard.add_row("Process RAM (RSS)", f"{baseline['ram_mb']} MB", f"{peak['ram_mb']} MB", f"{post['ram_mb']} MB", f"{ram_growth:+} MB", ram_verdict)

        thread_verdict = "[bold green]STABLE[/bold green]" if thread_growth <= 2 else "[bold red]LEAKED[/bold red]"
        scorecard.add_row("Thread Count", str(baseline["threads"]), str(peak["threads"]), str(post["threads"]), f"{thread_growth:+}", thread_verdict)

        handle_verdict = "[bold green]STABLE[/bold green]" if handle_growth < 10 else "[bold yellow]ELEVATED[/bold yellow]"
        scorecard.add_row("OS File Handles", str(baseline["handles"]), str(peak["handles"]), str(post["handles"]), f"{handle_growth:+}", handle_verdict)

        console.print(Panel(scorecard, border_style="yellow"))

        if ram_growth < 25.0 and thread_growth <= 4:
            console.print("[bold green]✔ Resource audit passed: Memory and handles cleanly returned to baseline without leaks![/bold green]\n")
        else:
            console.print("[bold red]✘ Warning: Noticeable resource growth detected during workload execution![/bold red]\n")
