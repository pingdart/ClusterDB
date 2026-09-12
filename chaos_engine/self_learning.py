import json
import os
import time
from typing import Any, Dict, List, Optional
from rich.console import Console
from rich.table import Table
from rich.panel import Panel

console = Console(safe_box=True)

LEDGER_DIR = os.path.join(".", "tests", "bugs")
LEDGER_FILE = os.path.join(LEDGER_DIR, "bug_ledger.json")
GENERATED_DIR = os.path.join(".", "tests", "generated")

class SelfLearningEngine:
    def __init__(self):
        os.makedirs(LEDGER_DIR, exist_ok=True)
        os.makedirs(GENERATED_DIR, exist_ok=True)
        self.ledger: List[Dict[str, Any]] = self._load_ledger()

    def _load_ledger(self) -> List[Dict[str, Any]]:
        if os.path.exists(LEDGER_FILE):
            try:
                with open(LEDGER_FILE, "r", encoding="utf-8") as f:
                    return json.load(f)
            except Exception:
                return []
        return []

    def _save_ledger(self):
        with open(LEDGER_FILE, "w", encoding="utf-8") as f:
            json.dump(self.ledger, f, indent=2)

    def record_bug(self, layer: str, title: str, query: str, error: str, repro_steps: List[str]) -> str:
        # Check if identical bug already recorded
        for b in self.ledger:
            if b["layer"] == layer and b["query"] == query and b["error"] == error:
                return b["bug_id"]

        bug_seq = len(self.ledger) + 1
        year = time.strftime("%Y")
        bug_id = f"BUG-{year}-{bug_seq:04d}"

        bug_entry = {
            "bug_id": bug_id,
            "layer": layer,
            "title": title,
            "query": query,
            "error": error,
            "timestamp": time.strftime("%Y-%m-%d %H:%M:%S"),
            "repro_steps": repro_steps,
            "status": "OPEN",
            "test_file": f"test_{bug_id.lower().replace('-', '_')}.py"
        }

        self.ledger.append(bug_entry)
        self._save_ledger()

        # Generate automated regression test file
        self._synthesize_regression_test(bug_entry)
        return bug_id

    def _synthesize_regression_test(self, bug: Dict[str, Any]):
        filename = bug["test_file"]
        filepath = os.path.join(GENERATED_DIR, filename)

        code = f'''#!/usr/bin/env python3
"""
Autonomous Regression Test: {bug['bug_id']}
Discovered by: Layer {bug['layer']}
Title: {bug['title']}
Discovered on: {bug['timestamp']}
"""

import requests
import pytest

BASE_URL = "http://127.0.0.1:4650"

def test_{bug['bug_id'].lower().replace('-', '_')}():
    session = requests.Session()
    # 1. Authenticate
    login_res = session.post(f"{{BASE_URL}}/login", json={{"username": "admin", "password": "admin123"}}, timeout=5)
    assert login_res.status_code == 200, "Login failed"
    token = login_res.json()["token"]

    # 2. Reproduction query
    query = """{bug['query'].replace('"', '\\"')}"""
    
    # 3. Execute
    res = session.post(f"{{BASE_URL}}/query", json={{"token": token, "query": query}}, timeout=10)
    
    # Assert database does not crash (HTTP 200/400 expected, never 500 internal server crash or connection refusal)
    assert res.status_code in [200, 400, 401, 403], f"Unexpected status code: {{res.status_code}}"
    data = res.json()
    assert "status" in data, "Malformed server response without status"

if __name__ == "__main__":
    test_{bug['bug_id'].lower().replace('-', '_')}()
    print("Regression test passed!")
'''
        with open(filepath, "w", encoding="utf-8") as f:
            f.write(code)

    def print_summary(self):
        if not self.ledger:
            console.print("[green]Self-Learning Ledger: No active bugs recorded. All systems green![/green]")
            return

        table = Table(title="[bold red]Self-Learning Bug Ledger & Regression Catalog[/bold red]", box=None, padding=(0, 1))
        table.add_column("Bug ID", style="bold cyan")
        table.add_column("Layer", style="yellow")
        table.add_column("Issue Title", style="white")
        table.add_column("Auto-Generated Test", style="green")
        table.add_column("Status", style="bold red")

        for b in self.ledger[-10:]:
            table.add_row(b["bug_id"], b["layer"], b["title"][:40], b["test_file"], b["status"])

        console.print(Panel(table, border_style="red", title="[bold white]Layer 6: Self-Learning Bug Ledger[/bold white]"))
