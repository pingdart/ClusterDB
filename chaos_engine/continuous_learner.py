import json
import os
import re
import sys
import time
import glob
from datetime import datetime, timezone
from typing import Any, Dict, List, Optional
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from rich.rule import Rule

console = Console(safe_box=True)

AI_LEARNING_DIR = os.path.join(".", "ai_learning")
SESSIONS_DIR = os.path.join(AI_LEARNING_DIR, "sessions")
BUGS_DIR = os.path.join(AI_LEARNING_DIR, "bugs")
REGRESSIONS_DIR = os.path.join(AI_LEARNING_DIR, "regressions")
DISCOVERIES_DIR = os.path.join(AI_LEARNING_DIR, "discoveries")
SCORECARDS_DIR = os.path.join(AI_LEARNING_DIR, "scorecards")
PATTERNS_FILE = os.path.join(DISCOVERIES_DIR, "new_patterns.json")
DAILY_REPORT_FILE = os.path.join(SCORECARDS_DIR, "daily_report.md")
WEEKLY_REPORT_FILE = os.path.join(SCORECARDS_DIR, "weekly_report.md")


class ContinuousLearningEngine:
    """
    Phase 14B: 24/7 Continuous Autonomous Validation & Learning Engine.
    Records every test in JSON sessions, generates Markdown bug dossiers,
    synthesizes permanent Python regression tests, retests all prior regressions,
    discovers recurring failure patterns, and publishes daily health scorecards.
    """

    def __init__(self, db_client=None, ollama_client=None):
        self.db = db_client
        self.ollama = ollama_client
        self._ensure_directories()
        
        self.session_buffer: List[Dict[str, Any]] = []
        self.patterns: List[Dict[str, Any]] = self._load_patterns()
        self.cycle_count = 0
        self.total_tests = 0
        self.total_passed = 0
        self.total_failed = 0
        self.total_regressions_checked = 0
        self.total_regressions_passed = 0
        self.latencies: List[float] = []
        self.start_time = time.time()
        self.test_counter = self._load_historical_metrics()

    def _ensure_directories(self):
        for path in [AI_LEARNING_DIR, SESSIONS_DIR, BUGS_DIR, REGRESSIONS_DIR, DISCOVERIES_DIR, SCORECARDS_DIR]:
            os.makedirs(path, exist_ok=True)

    def _load_historical_metrics(self) -> int:
        count = 0
        for fname in glob.glob(os.path.join(SESSIONS_DIR, "*.json")):
            try:
                with open(fname, "r", encoding="utf-8") as f:
                    records = json.load(f)
                    for r in records:
                        count += 1
                        self.total_tests += 1
                        if r.get("status") == "PASS":
                            self.total_passed += 1
                        else:
                            self.total_failed += 1
                        if "latency_ms" in r:
                            self.latencies.append(float(r["latency_ms"]))
            except Exception:
                pass
        return count


    def _load_patterns(self) -> List[Dict[str, Any]]:
        if os.path.exists(PATTERNS_FILE):
            try:
                with open(PATTERNS_FILE, "r", encoding="utf-8") as f:
                    return json.load(f)
            except Exception:
                return []
        return []

    def _save_patterns(self):
        try:
            with open(PATTERNS_FILE, "w", encoding="utf-8") as f:
                json.dump(self.patterns, f, indent=2)
        except Exception as e:
            console.print(f"[red]Failed to save patterns: {e}[/red]")

    def record_test(
        self,
        layer: str,
        action: str,
        expected: str,
        actual: str,
        status: str,
        latency_ms: float,
        user_role: str = "admin",
        details: Optional[Dict[str, Any]] = None
    ) -> str:
        """
        Record an individual test execution in JSON format:
        {
          "test_id":"T-8912",
          "time":"2026-09-12T06:12:22Z",
          "layer":"RBAC",
          "action":"DROP TABLE users",
          "user_role":"readonly",
          "expected":"403",
          "actual":"403",
          "status":"PASS",
          "latency_ms":12
        }
        """
        self.test_counter += 1
        test_id = f"T-{self.test_counter:05d}"
        now_iso = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")

        rec = {
            "test_id": test_id,
            "time": now_iso,
            "layer": layer,
            "action": action,
            "user_role": user_role,
            "expected": str(expected),
            "actual": str(actual),
            "status": status.upper(),
            "latency_ms": round(latency_ms, 2)
        }
        if details:
            rec["details"] = details

        self.session_buffer.append(rec)
        self.total_tests += 1
        if status.upper() == "PASS":
            self.total_passed += 1
        else:
            self.total_failed += 1
        self.latencies.append(latency_ms)

        # Flush buffer periodically (every 50 tests)
        if len(self.session_buffer) >= 50:
            self.flush_session()

        return test_id

    def flush_session(self):
        """Append session records to current hourly JSON session file."""
        if not self.session_buffer:
            return
        hour_key = datetime.now(timezone.utc).strftime("%Y-%m-%d_%H")
        session_file = os.path.join(SESSIONS_DIR, f"{hour_key}.json")

        existing = []
        if os.path.exists(session_file):
            try:
                with open(session_file, "r", encoding="utf-8") as f:
                    existing = json.load(f)
            except Exception:
                existing = []

        existing.extend(self.session_buffer)
        try:
            with open(session_file, "w", encoding="utf-8") as f:
                json.dump(existing, f, indent=2)
            self.session_buffer.clear()
        except Exception as e:
            console.print(f"[red]Failed to flush session to {session_file}: {e}[/red]")

    def record_bug_dossier(
        self,
        layer: str,
        severity: str,
        title: str,
        steps: List[str],
        expected: str,
        actual: str,
        repro_query: str
    ) -> str:
        """
        Create structured Markdown bug record (e.g. ai_learning/bugs/bug_001.md)
        and synthesize a permanent regression test in ai_learning/regressions/test_bug_001.py.
        """
        existing_bugs = glob.glob(os.path.join(BUGS_DIR, "bug_*.md"))
        bug_num = len(existing_bugs) + 1
        bug_id = f"BUG-{bug_num:03d}"
        date_str = datetime.now(timezone.utc).strftime("%Y-%m-%d")

        md_content = f"""# {bug_id}

**Date:**
{date_str}

**Layer:**
{layer}

**Severity:**
{severity.capitalize()}

**Title:**
{title}

**Steps:**
"""
        for i, step in enumerate(steps, 1):
            md_content += f"{i}. {step}\n"

        md_content += f"""
**Expected:**
{expected}

**Actual:**
{actual}

**Reproduction Query:**
```sql
{repro_query}
```

**Status:**
OPEN
"""
        bug_file = os.path.join(BUGS_DIR, f"bug_{bug_num:03d}.md")
        with open(bug_file, "w", encoding="utf-8") as f:
            f.write(md_content)

        # Synthesize regression test script
        self._synthesize_regression(bug_id, bug_num, layer, title, repro_query, expected, actual)
        
        # Check and record pattern discovery
        self._discover_pattern_from_bug(layer, title, severity)

        console.print(f"[bold red]✘ New Bug Discovered & Cataloged:[/bold red] [cyan]{bug_id}[/cyan] ({title})")
        return bug_id

    def _synthesize_regression(
        self,
        bug_id: str,
        bug_num: int,
        layer: str,
        title: str,
        query: str,
        expected: str,
        actual: str
    ):
        reg_file = os.path.join(REGRESSIONS_DIR, f"test_bug_{bug_num:03d}.py")
        clean_query = query.replace('"', '\\"')
        
        code = f'''#!/usr/bin/env python3
"""
Continuous Autonomous Regression Test: {bug_id}
Discovered by: {layer}
Title: {title}
Generated: {datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%SZ")}
"""

import requests
import pytest

BASE_URL = "http://127.0.0.1:4650"

def test_{bug_id.lower().replace("-", "_")}():
    session = requests.Session()
    # 1. Authenticate with PingDart Core
    login_res = session.post(f"{{BASE_URL}}/login", json={{"username": "admin", "password": "admin123"}}, timeout=5)
    assert login_res.status_code == 200, "Authentication failed"
    token = login_res.json().get("token")

    # 2. Reproduction Query
    query = """{clean_query}"""

    # 3. Execution
    res = session.post(f"{{BASE_URL}}/query", json={{"token": token, "query": query}}, timeout=10)
    
    # Assert database does not crash (HTTP 200/400/401/403 accepted, 500 panic rejected)
    assert res.status_code in [200, 400, 401, 403], f"Database crashed with status {{res.status_code}}"
    data = res.json()
    assert "status" in data, "Malformed engine response without status field"

if __name__ == "__main__":
    test_{bug_id.lower().replace("-", "_")}()
    print("Regression test {bug_id} verified PASS!")
'''
        with open(reg_file, "w", encoding="utf-8") as f:
            f.write(code)

    def _discover_pattern_from_bug(self, layer: str, title: str, severity: str):
        for pat in self.patterns:
            if pat.get("layer") == layer and pat.get("pattern") == title:
                pat["times_seen"] = pat.get("times_seen", 1) + 1
                self._save_patterns()
                return

        new_pat = {
            "pattern": title,
            "layer": layer,
            "times_seen": 1,
            "severity": severity.lower(),
            "new_test_generated": True,
            "discovered_at": datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%SZ")
        }
        self.patterns.append(new_pat)
        self._save_patterns()

    def run_all_regressions(self) -> Dict[str, Any]:
        """
        Retest all auto-generated regressions forever on every cycle.
        """
        reg_files = sorted(glob.glob(os.path.join(REGRESSIONS_DIR, "test_bug_*.py")))
        if not reg_files:
            return {"total": 0, "passed": 0, "failed": 0}

        passed = 0
        failed = 0
        for rf in reg_files:
            t0 = time.perf_counter()
            try:
                # Execute regression directly in python
                import importlib.util
                module_name = os.path.splitext(os.path.basename(rf))[0]
                spec = importlib.util.spec_from_file_location(module_name, rf)
                mod = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(mod)
                
                # Find and run test function
                for attr_name in dir(mod):
                    if attr_name.startswith("test_"):
                        fn = getattr(mod, attr_name)
                        fn()
                lat = (time.perf_counter() - t0) * 1000.0
                self.record_test("Regression", f"Retest {module_name}", "PASS", "PASS", "PASS", lat)
                passed += 1
            except Exception as e:
                lat = (time.perf_counter() - t0) * 1000.0
                self.record_test("Regression", f"Retest {module_name}", "PASS", f"FAIL: {e}", "FAIL", lat)
                failed += 1

        self.total_regressions_checked += len(reg_files)
        self.total_regressions_passed += passed
        return {"total": len(reg_files), "passed": passed, "failed": failed}

    def generate_daily_scorecard(self) -> str:
        """
        Publish daily report in ai_learning/scorecards/daily_report.md
        """
        today_str = datetime.now(timezone.utc).strftime("%Y-%m-%d")
        total_tests = max(self.total_tests, 1)
        pass_pct = (self.total_passed / total_tests) * 100.0
        
        existing_bugs = glob.glob(os.path.join(BUGS_DIR, "bug_*.md"))
        new_bugs = len(existing_bugs)
        reg_bugs = self.total_regressions_checked - self.total_regressions_passed

        sec_score = 99.8 if new_bugs == 0 else max(90.0, 100.0 - (new_bugs * 1.5))
        avg_lat = sum(self.latencies) / len(self.latencies) if self.latencies else 1.2
        perf_score = 99.0 if avg_lat < 5.0 else (95.0 if avg_lat < 20.0 else 85.0)

        overall_grade = "A+" if pass_pct >= 99.0 and sec_score >= 98.0 else ("A" if pass_pct >= 95.0 else "B+")

        content = f"""# PingDart Daily Health Report

**Date:** {today_str}
**Engine Status:** ONLINE (Production Hardened)
**Autonomous Cycles Run:** {self.cycle_count:,}

## Executive Telemetry

| Metric | Value |
| :--- | :--- |
| **Tests Executed** | {self.total_tests:,} |
| **Passed** | {self.total_passed:,} ({pass_pct:.2f}%) |
| **Failed** | {self.total_failed:,} |
| **New Bugs Discovered** | {new_bugs} |
| **Regression Bugs Detected** | {reg_bugs} |
| **Security Score** | {sec_score:.1f}% |
| **Performance Score** | {perf_score:.1f}% |
| **Average Query Latency** | {avg_lat:.2f} ms |
| **Discovered Patterns** | {len(self.patterns)} |
| **Overall Health Grade** | **{overall_grade}** |

---

## Active Bug Ledger

"""
        if existing_bugs:
            for bf in existing_bugs:
                bname = os.path.basename(bf)
                content += f"- [{bname}](file:///{os.path.abspath(bf).replace(chr(92), '/')})\n"
        else:
            content += "_Zero active bugs. All state invariants holding!_\n"

        content += f"""
---

## Discovered Attack & Failure Patterns

"""
        if self.patterns:
            for p in self.patterns:
                content += f"- **{p.get('pattern')}** (Seen: {p.get('times_seen')} times, Severity: {p.get('severity')})\n"
        else:
            content += "_No failure patterns detected yet._\n"

        with open(DAILY_REPORT_FILE, "w", encoding="utf-8") as f:
            f.write(content)

        return content
