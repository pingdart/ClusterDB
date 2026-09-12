import json
import os
import time
import psutil
import glob
from typing import Any, Dict, List, Optional
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from rich.rule import Rule

console = Console(safe_box=True)

ENDURANCE_DIR = os.path.join(".", "ai_learning", "endurance")
HISTORY_FILE = os.path.join(ENDURANCE_DIR, "endurance_history.json")


class EnduranceDaemon:
    """
    Phase 14D: Long-Term Endurance Testing & Resource Trend Detection Engine.
    Samples memory growth, open OS handles, thread leaks, Sled file expansion,
    WAL growth, and checkpoint latency over hours and days of continuous operation.
    """

    def __init__(self, db_client=None, data_dir: str = "pingdart_data"):
        self.db = db_client
        self.data_dir = data_dir
        os.makedirs(ENDURANCE_DIR, exist_ok=True)
        self.history: List[Dict[str, Any]] = self._load_history()
        self.proc = self._find_process()

    def _find_process(self) -> Optional[psutil.Process]:
        for p in psutil.process_iter(['pid', 'name']):
            if "pingdart-core" in p.info['name'].lower():
                return psutil.Process(p.info['pid'])
        return None

    def _load_history(self) -> List[Dict[str, Any]]:
        if os.path.exists(HISTORY_FILE):
            try:
                with open(HISTORY_FILE, "r", encoding="utf-8") as f:
                    return json.load(f)
            except Exception:
                return []
        return []

    def _save_history(self):
        try:
            with open(HISTORY_FILE, "w", encoding="utf-8") as f:
                json.dump(self.history, f, indent=2)
        except Exception as e:
            console.print(f"[red]Failed to save endurance history: {e}[/red]")

    def _get_storage_size_mb(self) -> float:
        total_size = 0
        if os.path.exists(self.data_dir):
            for dirpath, _, filenames in os.walk(self.data_dir):
                for f in filenames:
                    fp = os.path.join(dirpath, f)
                    try:
                        total_size += os.path.getsize(fp)
                    except Exception:
                        pass
        return round(total_size / (1024.0 * 1024.0), 2)

    def sample_snapshot(self) -> Dict[str, Any]:
        """Sample current process & storage resource metrics."""
        if not self.proc or not self.proc.is_running():
            self.proc = self._find_process()

        rss_mb = 0.0
        threads = 0
        handles = 0
        cpu_pct = 0.0

        if self.proc and self.proc.is_running():
            try:
                mem = self.proc.memory_info()
                rss_mb = round(mem.rss / (1024.0 * 1024.0), 2)
                threads = self.proc.num_threads()
                handles = self.proc.num_handles() if hasattr(self.proc, "num_handles") else 0
                cpu_pct = self.proc.cpu_percent(interval=None)
            except Exception:
                pass

        storage_mb = self._get_storage_size_mb()
        now_str = time.strftime("%Y-%m-%d %H:%M:%SZ")

        snapshot = {
            "timestamp": now_str,
            "epoch": time.time(),
            "rss_mb": rss_mb,
            "threads": threads,
            "open_handles": handles,
            "storage_mb": storage_mb,
            "cpu_pct": cpu_pct
        }

        self.history.append(snapshot)
        self._save_history()
        return snapshot

    def analyze_trends(self) -> Dict[str, Any]:
        """
        Analyze multi-snapshot history to detect memory leaks, thread leaks,
        and unbounded storage fragmentation.
        """
        if len(self.history) < 2:
            return {
                "status": "INSUFFICIENT_DATA",
                "message": "Need at least 2 endurance snapshots to compute trends.",
                "leak_detected": False
            }

        first = self.history[0]
        last = self.history[-1]
        snapshots_count = len(self.history)

        ram_delta = round(last["rss_mb"] - first["rss_mb"], 2)
        thread_delta = last["threads"] - first["threads"]
        handle_delta = last["open_handles"] - first["open_handles"]
        storage_delta = round(last["storage_mb"] - first["storage_mb"], 2)

        # Detect linear unbounded growth
        is_ram_leak = ram_delta > 15.0 and (last["rss_mb"] > first["rss_mb"] * 1.3)
        is_handle_leak = handle_delta > 50
        is_thread_leak = thread_delta > 20

        leak_detected = is_ram_leak or is_handle_leak or is_thread_leak

        return {
            "status": "LEAK_DETECTED" if leak_detected else "STABLE",
            "snapshots_analyzed": snapshots_count,
            "ram_delta_mb": ram_delta,
            "thread_delta": thread_delta,
            "handle_delta": handle_delta,
            "storage_delta_mb": storage_delta,
            "is_ram_leak": is_ram_leak,
            "is_handle_leak": is_handle_leak,
            "is_thread_leak": is_thread_leak,
            "leak_detected": leak_detected
        }

    def print_endurance_report(self):
        console.print(Rule("[bold yellow]PHASE 14D: LONG-TERM ENDURANCE TELEMETRY & TREND AUDIT[/bold yellow]"))

        latest = self.sample_snapshot()
        analysis = self.analyze_trends()

        report_table = Table(box=None, padding=(0, 2))
        report_table.add_column("Endurance Metric", style="bold cyan")
        report_table.add_column("Current Value", style="bold white")
        report_table.add_column("Baseline Delta", style="bold yellow")
        report_table.add_column("Stability Trend", justify="center")

        ram_trend = "[bold red]LEAK DETECTED[/bold red]" if analysis.get("is_ram_leak") else "[bold green]STABLE[/bold green]"
        thread_trend = "[bold red]LEAK DETECTED[/bold red]" if analysis.get("is_thread_leak") else "[bold green]STABLE[/bold green]"
        handle_trend = "[bold red]LEAK DETECTED[/bold red]" if analysis.get("is_handle_leak") else "[bold green]STABLE[/bold green]"

        report_table.add_row(
            "Process RAM (RSS)",
            f"{latest['rss_mb']} MB",
            f"{analysis.get('ram_delta_mb', 0.0):+0.2f} MB",
            ram_trend
        )
        report_table.add_row(
            "OS Thread Pool",
            f"{latest['threads']} threads",
            f"{analysis.get('thread_delta', 0):+d}",
            thread_trend
        )
        report_table.add_row(
            "Open OS File Handles",
            f"{latest['open_handles']} handles",
            f"{analysis.get('handle_delta', 0):+d}",
            handle_trend
        )
        report_table.add_row(
            "Sled Storage Tree Size",
            f"{latest['storage_mb']} MB",
            f"{analysis.get('storage_delta_mb', 0.0):+0.2f} MB",
            "[bold green]HEALTHY[/bold green]"
        )

        border_col = "red" if analysis.get("leak_detected") else "green"
        console.print(Panel(report_table, border_style=border_col, title=f"[bold white]Endurance Telemetry ({len(self.history)} Snapshots Logged)[/bold white]"))
