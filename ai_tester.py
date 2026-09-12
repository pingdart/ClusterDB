#!/usr/bin/env python3
"""
PingDart AI Tester - Powered by Local Ollama
A rich terminal-based autonomous and interactive database testing agent.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import time
from typing import Any, Dict, List, Optional, Tuple
import requests

# Ensure UTF-8 output on Windows console
if sys.platform == "win32":
    try:
        sys.stdout.reconfigure(encoding="utf-8")
        sys.stderr.reconfigure(encoding="utf-8")
    except Exception:
        pass

import random
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from rich.progress import Progress, SpinnerColumn, TextColumn, BarColumn, TimeRemainingColumn
from rich.syntax import Syntax
from rich.markdown import Markdown
from rich.text import Text
from rich.prompt import Prompt
from rich.columns import Columns
from rich.rule import Rule
from rich.live import Live
from rich.layout import Layout

# Phase 14: Autonomous Validation & Chaos Engine Modules
from chaos_engine.orchestrator import ChaosOrchestrator
from chaos_engine.self_learning import SelfLearningEngine
from chaos_engine.fuzzer import SQLFuzzer
from chaos_engine.simulator import UserBehaviorSimulator
from chaos_engine.rbac_attacker import RBACAttackEngine
from chaos_engine.destroyer import DatabaseDestroyer
from chaos_engine.torture import ImportExportTorture
from chaos_engine.red_team import SecurityRedTeam
from chaos_engine.explorer import AutonomousExplorer
from chaos_engine.state_validator import StateInvariantValidator
from chaos_engine.resource_auditor import ResourceAuditor
from chaos_engine.monkey import MonkeyTester
from chaos_engine.dba_auditor import AIDBAAuditor
from chaos_engine.continuous_daemon import ContinuousDaemon
from chaos_engine.continuous_learner import ContinuousLearningEngine
from chaos_engine.coverage_intel import CoverageIntelligenceEngine
from chaos_engine.endurance_daemon import EnduranceDaemon
from chaos_engine.plan_regression import QueryPlanRegressionDetector
from chaos_engine.root_cause_ai import AIRootCauseAnalyzer
from chaos_engine.cluster_chaos import ClusterChaosEngine

console = Console(safe_box=True)


PINGDART_DEFAULT_URL = "http://127.0.0.1:4650"
OLLAMA_DEFAULT_URL = "http://127.0.0.1:11434"
DEFAULT_DB_USER = "admin"
DEFAULT_DB_PASS = "admin123"


ASCII_BANNER = r"""
 [bold cyan]  ____  _             ____             _      [/bold cyan][bold magenta]    _    ___   [/bold magenta]
 [bold cyan] |  _ \(_)_ __   __ _|  _ \  __ _ _ __| |_    [/bold cyan][bold magenta]   / \  |_ _|  [/bold magenta]
 [bold cyan] | |_) | | '_ \ / _` | | | |/ _` | '__| __|   [/bold cyan][bold magenta]  / _ \  | |   [/bold magenta]
 [bold cyan] |  __/| | | | | (_| | |_| | (_| | |  | |_    [/bold cyan][bold magenta] / ___ \ | |   [/bold magenta]
 [bold cyan] |_|   |_|_| |_|\__, |____/ \__,_|_|   \__|   [/bold cyan][bold magenta]/_/   \_\___|  [/bold magenta]
 [bold cyan]                |___/                          [/bold cyan]
        [bold yellow]Autonomous & Interactive Tester (Ollama Engine)[/bold yellow]
"""

DB_DIALECT_SPEC = """
PingDartDB SQL Dialect Guide:
- CREATE DATABASE <db_name>;
- USE <db_name>;
- CREATE TABLE <table_name> (col1 INT PRIMARY KEY, col2 VARCHAR(100), col3 FLOAT, col4 BOOLEAN);
- INSERT INTO <table_name> (col1, col2, ...) VALUES (val1, val2, ...);
- SELECT * FROM <table_name> WHERE col = val;
- SELECT col1, col2 FROM <table_name> WHERE col > val ORDER BY col ASC LIMIT 10;
- UPDATE <table_name> SET col = val WHERE col2 = val2;
- DELETE FROM <table_name> WHERE col = val;
- CREATE INDEX <index_name> ON <table_name> (col);
- SHOW TABLES;
- SHOW DATABASES;
- DROP TABLE <table_name>;
- Multi-Tenant: CREATE TENANT <tenant_id>;
"""

class OllamaClient:
    def __init__(self, base_url: str = OLLAMA_DEFAULT_URL, model: Optional[str] = None):
        self.base_url = base_url.rstrip("/")
        self.model = model
        self.session = requests.Session()

    def check_connection(self) -> Tuple[bool, List[str]]:
        try:
            res = self.session.get(f"{self.base_url}/api/tags", timeout=3)
            if res.status_code == 200:
                models = [m["name"] for m in res.json().get("models", [])]
                if not self.model and models:
                    preferred = ["deepseek-coder:latest", "qwen2.5-coder:latest", "llama3.2:latest", "llama3:latest"]
                    for p in preferred:
                        if p in models:
                            self.model = p
                            break
                    if not self.model:
                        self.model = models[0]
                return True, models
            return False, []
        except Exception:
            return False, []

    def generate(self, prompt: str, system_prompt: Optional[str] = None) -> str:
        url = f"{self.base_url}/api/generate"
        payload: Dict[str, Any] = {
            "model": self.model,
            "prompt": prompt,
            "stream": False,
            "options": {
                "temperature": 0.2,
                "top_p": 0.9,
            }
        }
        if system_prompt:
            payload["system"] = system_prompt

        res = self.session.post(url, json=payload, timeout=60)
        res.raise_for_status()
        data = res.json()
        return data.get("response", "")

    def stream_generate(self, prompt: str, system_prompt: Optional[str] = None):
        url = f"{self.base_url}/api/generate"
        payload: Dict[str, Any] = {
            "model": self.model,
            "prompt": prompt,
            "stream": True,
            "options": {
                "temperature": 0.3,
            }
        }
        if system_prompt:
            payload["system"] = system_prompt

        with self.session.post(url, json=payload, stream=True, timeout=60) as resp:
            resp.raise_for_status()
            for line in resp.iter_lines():
                if line:
                    chunk = json.loads(line.decode("utf-8"))
                    yield chunk.get("response", "")
                    if chunk.get("done", False):
                        break


class PingDartClient:
    def __init__(self, base_url: str = PINGDART_DEFAULT_URL):
        self.base_url = base_url.rstrip("/")
        self.token: Optional[str] = None
        self.current_db: Optional[str] = None
        self.session = requests.Session()

    def is_running(self) -> bool:
        try:
            res = self.session.get(f"{self.base_url}/metrics", timeout=2)
            return res.status_code == 200
        except Exception:
            try:
                res = self.session.post(f"{self.base_url}/login", json={"username": "test", "password": "bad"}, timeout=2)
                return res.status_code in [200, 401]
            except Exception:
                return False

    def auto_start_db(self) -> bool:
        """Attempt to launch pingdart-core binary in background if not running."""
        exe_paths = [
            os.path.join(".", "target", "debug", "pingdart-core.exe"),
            os.path.join(".", "target", "release", "pingdart-core.exe"),
            os.path.join(".", "pingdart-core.exe"),
        ]
        exe_to_run = None
        for p in exe_paths:
            if os.path.exists(p):
                exe_to_run = p
                break

        if not exe_to_run:
            return False

        console.print(f"[bold yellow]Starting PingDart DB daemon from {exe_to_run}...[/bold yellow]")
        try:
            subprocess.Popen([exe_to_run], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            for _ in range(15):
                time.sleep(0.5)
                if self.is_running():
                    return True
            return False
        except Exception as e:
            console.print(f"[red]Failed to launch DB: {e}[/red]")
            return False

    def login(self, username: str = DEFAULT_DB_USER, password: str = DEFAULT_DB_PASS) -> bool:
        try:
            res = self.session.post(
                f"{self.base_url}/login",
                json={"username": username, "password": password},
                timeout=5
            )
            if res.status_code == 200:
                data = res.json()
                if data.get("status") == "success" and data.get("token"):
                    self.token = data["token"]
                    return True
            return False
        except Exception as e:
            console.print(f"[red]Login error: {e}[/red]")
            return False

    def execute_query(self, query: str, database: Optional[str] = None) -> Dict[str, Any]:
        target_db = database or self.current_db
        clean_q = query.strip()

        # Intercept USE command
        if clean_q.lower().startswith("use "):
            parts = clean_q.rstrip(";").split()
            if len(parts) == 2:
                self.current_db = parts[1]
                return {
                    "status": "success",
                    "result": f"Switched context to database '{self.current_db}'",
                    "error": None,
                    "latency_ms": 0.1,
                    "query": query
                }

        payload: Dict[str, Any] = {
            "token": self.token or "",
            "query": clean_q,
        }
        if target_db:
            payload["database"] = target_db

        t0 = time.perf_counter()
        try:
            res = self.session.post(f"{self.base_url}/query", json=payload, timeout=10)
            latency_ms = (time.perf_counter() - t0) * 1000.0
            data = res.json()
            data["latency_ms"] = round(latency_ms, 2)
            data["query"] = query
            return data
        except Exception as e:
            latency_ms = (time.perf_counter() - t0) * 1000.0
            return {
                "status": "error",
                "result": None,
                "error": str(e),
                "latency_ms": round(latency_ms, 2),
                "query": query
            }


class AITesterAgent:
    def __init__(self, ollama: OllamaClient, db: PingDartClient):
        self.ollama = ollama
        self.db = db
        self.history: List[Dict[str, Any]] = []
        self.learner = SelfLearningEngine()
        self.chaos_orchestrator = ChaosOrchestrator(self.db, self.ollama)

    def print_header(self, models: List[str]):
        console.clear()
        console.print(ASCII_BANNER)

        db_status = "[bold green]ONLINE [OK][/bold green]" if self.db.is_running() else "[bold red]OFFLINE [FAILED][/bold red]"
        llm_status = f"[bold cyan]{self.ollama.model}[/bold cyan] ([green]ACTIVE[/green])"

        table = Table(show_header=False, box=None, padding=(0, 2))
        table.add_row("[bold]PingDart Engine:[/bold]", db_status, f"[dim]{self.db.base_url}[/dim]")
        table.add_row("[bold]Ollama Model:[/bold]", llm_status, f"[dim]{self.ollama.base_url}[/dim]")
        table.add_row("[bold]Session Auth:[/bold]", f"[green]{'Authenticated (admin)' if self.db.token else 'Pending'}[/green]", f"[dim]DB: {self.db.current_db or 'None'}[/dim]")
        
        console.print(Panel(table, title="[bold white]System Status[/bold white]", border_style="cyan"))
        console.print()

    def run_single_test(self, test_case: Dict[str, Any], index: int, total: int) -> Dict[str, Any]:
        category = test_case.get("category", "SQL")
        desc = test_case.get("description", "Execute test query")
        sql = test_case.get("query", "").strip()
        expected_success = test_case.get("expected_success", True)

        console.print(f"[bold cyan]=== [Test {index}/{total}] [{category}] {desc} ===[/bold cyan]")
        console.print(Syntax(sql, "sql", theme="monokai", line_numbers=False, word_wrap=True))

        with console.status(f"[bold green]Executing query on PingDartDB...[/bold green]", spinner="dots"):
            result = self.db.execute_query(sql)

        is_success = result.get("status") == "success"
        err_msg = result.get("error")
        res_msg = result.get("result")
        lat = result.get("latency_ms", 0.0)

        # Evaluate pass/fail based on expectation
        passed = (is_success == expected_success)
        status_badge = "[bold green][PASS][/bold green]" if passed else "[bold red][FAIL][/bold red]"
        
        info_table = Table(show_header=False, box=None, padding=(0, 1))
        info_table.add_row("[bold]Verdict:[/bold]", status_badge)
        info_table.add_row("[bold]Latency:[/bold]", f"[yellow]{lat:.2f} ms[/yellow]")
        if is_success:
            res_str = str(res_msg)
            if len(res_str) > 300:
                res_str = res_str[:300] + "... [truncated]"
            info_table.add_row("[bold]Result:[/bold]", f"[green]{res_str}[/green]")
        else:
            info_table.add_row("[bold]Error Output:[/bold]", f"[red]{err_msg}[/red]")

        border_color = "green" if passed else "red"
        console.print(Panel(info_table, border_style=border_color, padding=(0, 1)))
        console.print()

        record = {
            "index": index,
            "category": category,
            "description": desc,
            "query": sql,
            "expected_success": expected_success,
            "actual_success": is_success,
            "passed": passed,
            "latency_ms": lat,
            "result": res_msg,
            "error": err_msg
        }
        self.history.append(record)
        return record

    def run_automated_suite(self):
        console.print(Rule("[bold magenta]Phase 1: Generating Test Matrix with Ollama AI[/bold magenta]"))
        
        prompt = f"""
You are an expert Database QA Architect. Generate an end-to-end automated test suite for PingDartDB.
Follow the dialect specification:
{DB_DIALECT_SPEC}

Generate a comprehensive test matrix consisting of 8-10 ordered SQL operations:
1. CREATE DATABASE ai_regression_db;
2. USE ai_regression_db;
3. CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(100), email VARCHAR(100), age INT, active BOOLEAN);
4. INSERT single row
5. INSERT another row with different values
6. SELECT with WHERE clause filter
7. UPDATE row
8. CREATE INDEX on column
9. Test an invalid syntax query (which is expected to fail gracefully with expected_success: false)
10. SELECT * with ORDER BY or LIMIT

Respond ONLY with a valid JSON array. Each element must have:
- "category": string (e.g. "DDL", "DML", "QUERY", "INDEX", "ERROR_HANDLING")
- "description": brief summary of test goal
- "query": single valid SQL query string
- "expected_success": boolean (true for normal queries, false for invalid ones)

No conversational text, only the raw JSON array.
"""
        with console.status("[bold cyan]Ollama is designing test scenarios...[/bold cyan]", spinner="bouncingBar"):
            ai_resp = self.ollama.generate(prompt)

        match = re.search(r'\[.*\]', ai_resp, re.DOTALL)
        test_cases = []
        if match:
            try:
                test_cases = json.loads(match.group(0))
            except Exception:
                pass

        if not test_cases:
            console.print("[yellow]Notice: LLM returned raw text, using built-in curated AI fallback suite...[/yellow]")
            test_cases = [
                {"category": "DDL", "description": "Create new dedicated test database", "query": "CREATE DATABASE ai_suite_db;", "expected_success": True},
                {"category": "CONTEXT", "description": "Switch active database context", "query": "USE ai_suite_db;", "expected_success": True},
                {"category": "DDL", "description": "Create users table with mixed types", "query": "CREATE TABLE employees (id INT PRIMARY KEY, name VARCHAR(100), department VARCHAR(50), salary FLOAT);", "expected_success": True},
                {"category": "DML", "description": "Insert employee record Alice", "query": "INSERT INTO employees (id, name, department, salary) VALUES (101, 'Alice Smith', 'Engineering', 95000.0);", "expected_success": True},
                {"category": "DML", "description": "Insert employee record Bob", "query": "INSERT INTO employees (id, name, department, salary) VALUES (102, 'Bob Jones', 'Product', 85000.0);", "expected_success": True},
                {"category": "DML", "description": "Insert employee record Charlie", "query": "INSERT INTO employees (id, name, department, salary) VALUES (103, 'Charlie Brown', 'Engineering', 92000.0);", "expected_success": True},
                {"category": "QUERY", "description": "Filter by department with WHERE", "query": "SELECT * FROM employees WHERE department = 'Engineering';", "expected_success": True},
                {"category": "INDEX", "description": "Create secondary index on department", "query": "CREATE INDEX idx_emp_dept ON employees (department);", "expected_success": True},
                {"category": "UPDATE", "description": "Update salary of specific employee", "query": "UPDATE employees SET salary = 99000.0 WHERE id = 101;", "expected_success": True},
                {"category": "NEGATIVE", "description": "Expect failure on malformed syntax", "query": "SELECT * FROM FROM INVALID ???;", "expected_success": False},
                {"category": "QUERY", "description": "Query with ORDER BY and LIMIT", "query": "SELECT name, salary FROM employees ORDER BY salary DESC LIMIT 2;", "expected_success": True},
                {"category": "CLEANUP", "description": "Delete record and verify deletion", "query": "DELETE FROM employees WHERE id = 102;", "expected_success": True},
            ]

        console.print(Rule(f"[bold green]Phase 2: Executing {len(test_cases)} Tests Against PingDartDB[/bold green]"))
        for i, tc in enumerate(test_cases, 1):
            self.run_single_test(tc, i, len(test_cases))
            time.sleep(0.15)

        self.print_final_summary()

    def run_fuzz_testing(self):
        console.print(Rule("[bold magenta]Phase 1: Ollama Generating Fuzz & Stress Test Cases[/bold magenta]"))
        prompt = f"""
You are a security and stress test engineer. Generate 8 tricky, boundary, and fuzz test queries against PingDartDB.
The queries should include:
- SQL injection attempts (e.g. ' OR '1'='1)
- Deeply nested or long string values
- Missing columns or non-existent tables
- Numeric overflows or boundary values
- Empty identifiers or special characters
- Malformed syntax to verify server does NOT panic and returns clean error.

Respond ONLY with a valid JSON array of objects:
- "category": "FUZZ" or "SECURITY" or "STRESS"
- "description": What edge case is being tested
- "query": The raw SQL query string
- "expected_success": boolean (usually false for malformed/illegal queries, true if the engine safely executes it)
"""
        with console.status("[bold magenta]Ollama generating fuzz mutations...[/bold magenta]", spinner="point"):
            ai_resp = self.ollama.generate(prompt)

        match = re.search(r'\[.*\]', ai_resp, re.DOTALL)
        test_cases = []
        if match:
            try:
                test_cases = json.loads(match.group(0))
            except Exception:
                pass

        if not test_cases:
            test_cases = [
                {"category": "FUZZ", "description": "Query non-existent table", "query": "SELECT * FROM ghost_table_xyz_123 WHERE id = 999;", "expected_success": False},
                {"category": "SECURITY", "description": "SQL injection string probe", "query": "SELECT * FROM employees WHERE name = 'Alice' OR '1'='1';", "expected_success": True},
                {"category": "FUZZ", "description": "Unclosed string boundary", "query": "INSERT INTO employees (id, name) VALUES (999, 'unclosed string);", "expected_success": False},
                {"category": "STRESS", "description": "Huge integer boundary test", "query": "INSERT INTO employees (id, name) VALUES (999999999999999999, 'Large ID');", "expected_success": True},
                {"category": "FUZZ", "description": "Special characters in query", "query": "SELECT [!@#$%^&*()] FROM employees;", "expected_success": False},
                {"category": "FUZZ", "description": "Empty statement execution", "query": ";;;;", "expected_success": False},
            ]

        console.print(Rule(f"[bold red]Phase 2: Executing {len(test_cases)} Fuzz Probes Against PingDartDB[/bold red]"))
        for i, tc in enumerate(test_cases, 1):
            self.run_single_test(tc, i, len(test_cases))
            time.sleep(0.15)

        self.print_final_summary()

    def run_interactive_repl(self):
        console.print(Panel(
            "[bold green]Interactive AI Autonomous Terminal (Phase 14 Enabled)[/bold green]\n"
            "• Natural Language: [italic]'test creating a products table with 5 items'[/italic]\n"
            "• Direct SQL: [italic]'SELECT * FROM employees'[/italic]\n"
            "• Autonomous Chaos Modes:\n"
            "   [bold magenta]'chaos'[/bold magenta]    : Run Full 12-Layer Autonomous Validation Battery\n"
            "   [cyan]'fuzz'[/cyan]     : Run Layer 1 AI SQL Fuzzer (Malformed & Boundary queries)\n"
            "   [yellow]'sim'[/yellow]      : Run Layer 2 Concurrent Multi-User Behavior Simulator\n"
            "   [red]'rbac'[/red]     : Run Layer 3 Tenant Isolation & RBAC Penetration Attack\n"
            "   [red]'destroy'[/red]  : Run Layer 4 Database Destroyer & Concurrency Crash Chaos\n"
            "   [magenta]'torture'[/magenta]  : Run Layer 5 Malformed JSON, CSV & Protobuf Torture\n"
            "   [red]'redteam'[/red]  : Run Layer 7 Security Red Team Penetration Testing\n"
            "   [blue]'explore'[/blue]  : Run Layer 8 Autonomous Explorer Blackbox Hacker Agent\n"
            "   [green]'state'[/green]    : Run Layer 9 State Invariant Silent Corruption Auditor\n"
            "   [yellow]'leak'[/yellow]     : Run Layer 10 RAM & OS Handle Leak Auditor\n"
            "   [cyan]'monkey'[/cyan]   : Run Layer 11 Long-Duration Monkey Tester with Latency Drift\n"
            "   [blue]'dba'[/blue]      : Run Layer 12 AI DBA Schema & Query Optimizer\n"
            "   [green]'bugs'[/green]     : View Layer 6 Self-Learning Bug Ledger & Regression Tests\n"
            "   [cyan]'bench'[/cyan]    : Run 1,000-Op Benchmark | [cyan]'loop'[/cyan]: Continuous Infinite Load\n"
            "   [dim]'exit'[/dim]     : Quit",
            border_style="green",
            title="[bold white]PingDart AI Terminal[/bold white]"
        ))

        while True:
            try:
                db_name = self.db.current_db or "none"
                user_input = Prompt.ask(f"[bold cyan]ai-tester[/bold cyan] [dim]({db_name})[/dim] >").strip()
                if not user_input:
                    continue
                if user_input.lower() in ["exit", "quit", "q"]:
                    console.print("[yellow]Exiting PingDart AI Tester. Goodbye![/yellow]")
                    break
                if user_input.lower() == "auto":
                    self.run_automated_suite()
                    continue
                if user_input.lower() == "chaos":
                    self.run_chaos_battery()
                    continue
                if user_input.lower() in ["fuzz", "fuzzer"]:
                    self.run_fuzzer(count=100)
                    continue
                if user_input.lower() in ["sim", "simulate", "simulator"]:
                    self.run_simulator(num_users=8, max_workers=4)
                    continue
                if user_input.lower() in ["rbac", "attack"]:
                    self.run_rbac_attacker()
                    continue
                if user_input.lower() in ["destroy", "destroyer"]:
                    self.run_destroyer()
                    continue
                if user_input.lower() in ["torture", "import-torture"]:
                    self.run_torture()
                    continue
                if user_input.lower() in ["redteam", "security"]:
                    self.run_red_team()
                    continue
                if user_input.lower() in ["explore", "explorer"]:
                    self.run_explorer(steps=10)
                    continue
                if user_input.lower() in ["state", "invariant"]:
                    self.run_state_validator()
                    continue
                if user_input.lower() in ["leak", "resource", "auditor"]:
                    self.run_resource_auditor(ops=150)
                    continue
                if user_input.lower() in ["monkey", "monkeytest"]:
                    self.run_monkey_testing(duration_sec=10)
                    continue
                if user_input.lower() in ["dba", "advisor"]:
                    self.run_dba_auditor()
                    continue
                if user_input.lower() in ["bugs", "ledger", "regressions"]:
                    self.show_bug_ledger()
                    continue
                if user_input.lower() in ["247", "daemon", "learn", "continuous-learning"]:
                    self.run_continuous_learning()
                    continue
                if user_input.lower() in ["scorecard", "daily", "report"]:
                    self.show_daily_scorecard()
                    continue
                if user_input.lower() in ["patterns", "discoveries"]:
                    self.show_discovered_patterns()
                    continue
                if user_input.lower() in ["coverage", "intel"]:
                    self.run_coverage_intel()
                    continue
                if user_input.lower() in ["endurance", "leak-audit"]:
                    self.run_endurance_daemon()
                    continue
                if user_input.lower() in ["plan", "plan-regression", "cbo"]:
                    self.run_plan_regression()
                    continue
                if user_input.lower() in ["rootcause", "patch", "repair"]:
                    self.run_root_cause_analyzer()
                    continue
                if user_input.lower() in ["cluster", "cluster-chaos", "raft"]:
                    self.run_cluster_chaos()
                    continue
                if user_input.lower().startswith("bench") or user_input.lower() == "benchmark":

                    parts = user_input.split()
                    cnt = int(parts[1]) if len(parts) > 1 and parts[1].isdigit() else 1000
                    self.run_continuous_benchmark(count=cnt)
                    continue
                if user_input.lower() in ["continuous", "loop", "infinite"]:
                    self.run_continuous_benchmark(continuous=True)
                    continue
                if user_input.lower() == "summary":
                    self.print_final_summary()
                    continue



                # Check if user typed a direct SQL statement
                sql_starters = ["select", "insert", "update", "delete", "create", "drop", "show", "use", "explain"]
                is_direct_sql = any(user_input.lower().startswith(s) for s in sql_starters)

                if is_direct_sql:
                    self.run_single_test({
                        "category": "DIRECT_SQL",
                        "description": "User direct query",
                        "query": user_input,
                        "expected_success": True
                    }, len(self.history) + 1, len(self.history) + 1)
                    continue

                # Otherwise, let Ollama interpret and plan the query
                console.print(f"[bold purple]Ollama Reasoning:[/bold purple] Generating SQL test for [italic]'{user_input}'[/italic]...")
                system_prompt = f"""
You are the AI testing engine for PingDartDB.
Dialect:
{DB_DIALECT_SPEC}

Translate the user's test goal into 1 or more SQL queries.
Return a JSON array of query items:
[{{"query": "SQL string", "description": "what this does", "expected_success": true}}]
Only return JSON.
"""
                with console.status("[bold cyan]Ollama is synthesizing query plan...[/bold cyan]", spinner="arc"):
                    resp = self.ollama.generate(f"User request: {user_input}", system_prompt=system_prompt)

                match = re.search(r'\[.*\]', resp, re.DOTALL)
                if match:
                    try:
                        queries = json.loads(match.group(0))
                        for q in queries:
                            self.run_single_test(q, len(self.history) + 1, len(self.history) + 1)
                        continue
                    except Exception:
                        pass

                # If no JSON array found, display model's response directly
                console.print(Panel(Markdown(resp), title="[bold cyan]Ollama Response[/bold cyan]", border_style="cyan"))

            except (KeyboardInterrupt, EOFError):
                console.print("\n[yellow]Session ended.[/yellow]")
                break

    def run_continuous_benchmark(self, count: int = 1000, continuous: bool = False, ai_interval: int = 50):
        """
        High-scale continuous stress testing and benchmarking engine.
        Creates databases, tables, indexes, and executes continuous INSERTs, SEARCHes, UPDATEs, DELETEs,
        with periodic AI-generated chaos mutations and live terminal dashboard.
        """
        console.clear()
        console.print(ASCII_BANNER)
        console.print(Rule("[bold magenta]PingDartDB High-Scale Continuous AI Benchmark[/bold magenta]"))
        target_str = "CONTINUOUS (Infinite - Press Ctrl+C to Stop)" if continuous else f"{count:,} Target Operations"
        console.print(f"[bold cyan]Mode:[/bold cyan] {target_str} | [bold cyan]AI Intervention:[/bold cyan] Every {ai_interval} ops")
        console.print("[dim]Generating databases, tables, inserts, searches, updates, and deletes...[/dim]\n")

        # Telemetry State
        start_time = time.time()
        ops_done = 0
        passed_ops = 0
        failed_ops = 0
        op_counts = {
            "CREATE_DB": 0,
            "CREATE_TABLE": 0,
            "CREATE_INDEX": 0,
            "INSERT": 0,
            "SEARCH": 0,
            "UPDATE": 0,
            "DELETE": 0,
            "AI_CHAOS": 0,
        }
        latencies: List[float] = []
        recent_events: List[Dict[str, Any]] = []
        failed_events: List[Dict[str, Any]] = []

        def extract_sql_query(raw_text: str) -> str:
            # 1. Look for ```sql ... ``` block
            match = re.search(r'```(?:sql)?\s*([\s\S]*?)\s*```', raw_text, re.IGNORECASE)
            if match:
                raw_text = match.group(1).strip()
            # 2. Filter out conversational text lines
            lines = [l.strip() for l in raw_text.splitlines() if l.strip()]
            starters = ("select", "insert", "update", "delete", "create", "drop", "alter", "show", "use")
            sql_lines = []
            for l in lines:
                if l.lower().startswith(starters) or sql_lines:
                    sql_lines.append(l)
                    if l.endswith(";"):
                        break
            if sql_lines:
                return " ".join(sql_lines).strip()
            cleaned = raw_text.strip()
            if not cleaned.endswith(";"):
                cleaned += ";"
            return cleaned

        # Database schema tracker
        dbs: List[str] = ["bench_scale_0"]
        tables: Dict[str, List[str]] = {"bench_scale_0": ["users", "orders"]}
        row_ids: Dict[str, List[int]] = {"bench_scale_0_users": [1], "bench_scale_0_orders": [1]}
        # Initialize next_id with a timestamp-based offset to prevent primary key collisions with previous runs
        next_id = int(time.time() * 10) % 1000000000 + 100

        # 1. Initial Seed Setup
        with console.status("[bold green]Seeding initial benchmark databases and tables...[/bold green]"):
            self.db.execute_query("CREATE DATABASE bench_scale_0;")
            self.db.current_db = "bench_scale_0"
            self.db.execute_query("CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(100), email VARCHAR(100), age INT, status VARCHAR(20));", "bench_scale_0")
            self.db.execute_query("CREATE TABLE orders (id INT PRIMARY KEY, user_id INT, amount FLOAT, status VARCHAR(20));", "bench_scale_0")
            self.db.execute_query("CREATE INDEX idx_users_status ON users (status);", "bench_scale_0")
            self.db.execute_query("INSERT INTO users (id, name, email, age, status) VALUES (1, 'User 1', 'user1@test.com', 25, 'active');", "bench_scale_0")
            self.db.execute_query("INSERT INTO orders (id, user_id, amount, status) VALUES (1, 1, 99.5, 'COMPLETED');", "bench_scale_0")
            op_counts["CREATE_DB"] += 1
            op_counts["CREATE_TABLE"] += 2
            op_counts["CREATE_INDEX"] += 1
            op_counts["INSERT"] += 2
            ops_done += 6
            passed_ops += 6

        def make_dashboard():
            elapsed = max(0.001, time.time() - start_time)
            qps = ops_done / elapsed
            avg_lat = sum(latencies) / len(latencies) if latencies else 0.0
            min_lat = min(latencies) if latencies else 0.0
            max_lat = max(latencies) if latencies else 0.0
            sorted_lat = sorted(latencies) if latencies else [0.0]
            p95_idx = min(len(sorted_lat) - 1, int(len(sorted_lat) * 0.95))
            p95_lat = sorted_lat[p95_idx]

            # Top Metrics Table
            top_table = Table(box=None, expand=True, padding=(0, 2))
            top_table.add_column("Progress", style="bold cyan")
            top_table.add_column("Throughput", style="bold green")
            top_table.add_column("Latency (Avg / P95)", style="bold yellow")
            top_table.add_column("Pass / Fail Rate", style="bold white")

            prog_str = f"{ops_done:,} ops (Continuous)" if continuous else f"{ops_done:,} / {count:,} ({min(100.0, (ops_done/count)*100):.1f}%)"
            top_table.add_row(
                prog_str,
                f"{qps:.1f} QPS",
                f"{avg_lat:.2f} ms / {p95_lat:.2f} ms",
                f"[green]{passed_ops:,} PASS[/green] | [red]{failed_ops} FAIL[/red]"
            )

            # Operations Breakdown Table
            breakdown = Table(title="[bold white]Operations Breakdown[/bold white]", box=None, padding=(0, 1), expand=True)
            breakdown.add_column("Databases", justify="center")
            breakdown.add_column("Tables", justify="center")
            breakdown.add_column("Inserts", justify="center", style="cyan")
            breakdown.add_column("Searches/Selects", justify="center", style="green")
            breakdown.add_column("Updates", justify="center", style="yellow")
            breakdown.add_column("Deletes", justify="center", style="red")
            breakdown.add_column("AI Mutations", justify="center", style="magenta")

            breakdown.add_row(
                f"{len(dbs)} DBs",
                f"{sum(len(t) for t in tables.values())} Tables",
                f"{op_counts['INSERT']:,}",
                f"{op_counts['SEARCH']:,}",
                f"{op_counts['UPDATE']:,}",
                f"{op_counts['DELETE']:,}",
                f"{op_counts['AI_CHAOS']:,}"
            )

            # Recent Live Logs Table
            log_table = Table(title="[bold white]Live Query Stream (Recent Activity)[/bold white]", box=None, padding=(0, 1), expand=True)
            log_table.add_column("Time", style="dim", width=8)
            log_table.add_column("Type", width=12)
            log_table.add_column("Database.Table", width=22)
            log_table.add_column("Status", width=8)
            log_table.add_column("Latency", justify="right", width=10)

            for evt in reversed(recent_events[-5:]):
                status_style = "[green][PASS][/green]" if evt["success"] else "[red][FAIL][/red]"
                log_table.add_row(
                    evt["time"],
                    f"[bold]{evt['type']}[/bold]",
                    evt["target"],
                    status_style,
                    f"{evt['lat']:.2f} ms"
                )

            layout = Table.grid(padding=1)
            layout.add_row(Panel(top_table, title="[bold white]Real-Time Performance Dashboard[/bold white]", border_style="cyan"))
            layout.add_row(Panel(breakdown, border_style="blue"))
            layout.add_row(Panel(log_table, border_style="magenta"))
            return layout

        try:
            with Live(make_dashboard(), refresh_per_second=4, console=console) as live:
                while continuous or ops_done < count:
                    target_db = random.choice(dbs)
                    target_tbl = random.choice(tables.get(target_db, ["users"]))
                    tbl_key = f"{target_db}_{target_tbl}"
                    executed_sql = ""

                    # Periodic AI Chaos & Query Mutation
                    if ops_done > 0 and ops_done % ai_interval == 0:
                        op_type = "AI_CHAOS"
                        prompt = f"Output ONLY one SQL SELECT or INSERT query for table {target_tbl} in database {target_db}. Example: SELECT * FROM {target_tbl} WHERE id = 1; Do not include explanation or markdown."
                        system_prompt = "You are a database SQL generator. Return ONLY the raw SQL statement ending in semicolon. No commentary."
                        try:
                            raw_ai = self.ollama.generate(prompt, system_prompt=system_prompt)
                            executed_sql = extract_sql_query(raw_ai)
                            res = self.db.execute_query(executed_sql, database=target_db)
                        except Exception as e:
                            executed_sql = f"SELECT * FROM {target_tbl};"
                            res = {"status": "error", "error": str(e), "latency_ms": 1.0}
                        q_target = f"{target_db}.{target_tbl} [AI]"
                    else:
                        # Weighted operations: 5% Scale DDL, 35% INSERT, 35% SEARCH, 15% UPDATE, 10% DELETE
                        dice = random.random()
                        if dice < 0.05:
                            # DDL Scale out: Create new DB or Table
                            if len(dbs) < 50 and random.random() < 0.5:
                                op_type = "CREATE_DB"
                                new_db = f"bench_scale_{len(dbs)}"
                                dbs.append(new_db)
                                tables[new_db] = ["users"]
                                row_ids[f"{new_db}_users"] = []
                                executed_sql = f"CREATE DATABASE {new_db};"
                                res = self.db.execute_query(executed_sql)
                                if res.get("status") == "success":
                                    self.db.execute_query(f"CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(100), email VARCHAR(100), age INT, status VARCHAR(20));", new_db)
                                q_target = f"{new_db} (new DB)"
                            else:
                                op_type = "CREATE_TABLE"
                                new_tbl = f"metric_{len(tables.get(target_db, []))}"
                                tables[target_db].append(new_tbl)
                                row_ids[f"{target_db}_{new_tbl}"] = []
                                executed_sql = f"CREATE TABLE {new_tbl} (id INT PRIMARY KEY, host VARCHAR(50), cpu FLOAT, memory FLOAT);"
                                res = self.db.execute_query(executed_sql, target_db)
                                q_target = f"{target_db}.{new_tbl}"
                        elif dice < 0.40:
                            # INSERT
                            op_type = "INSERT"
                            cur_id = next_id
                            next_id += 1
                            if "user" in target_tbl:
                                age = random.randint(18, 75)
                                status = random.choice(["active", "pending", "suspended", "verified"])
                                executed_sql = f"INSERT INTO {target_tbl} (id, name, email, age, status) VALUES ({cur_id}, 'User {cur_id}', 'user{cur_id}@test.com', {age}, '{status}');"
                            elif "order" in target_tbl:
                                amt = round(random.uniform(10.0, 999.0), 2)
                                status = random.choice(["NEW", "PAID", "SHIPPED", "COMPLETED"])
                                executed_sql = f"INSERT INTO {target_tbl} (id, user_id, amount, status) VALUES ({cur_id}, {random.randint(1, 100)}, {amt}, '{status}');"
                            else:
                                executed_sql = f"INSERT INTO {target_tbl} (id, host, cpu, memory) VALUES ({cur_id}, 'srv-{random.randint(1, 10)}', {round(random.uniform(10, 99), 1)}, {round(random.uniform(1024, 16384), 0)});"
                            res = self.db.execute_query(executed_sql, target_db)
                            if res.get("status") == "success":
                                if tbl_key not in row_ids:
                                    row_ids[tbl_key] = []
                                row_ids[tbl_key].append(cur_id)
                            q_target = f"{target_db}.{target_tbl}"
                        elif dice < 0.75:
                            # SEARCH / SELECT
                            op_type = "SEARCH"
                            existing = row_ids.get(tbl_key, [])
                            if existing and random.random() < 0.5:
                                # Primary Key Point Lookup
                                q_id = random.choice(existing)
                                executed_sql = f"SELECT * FROM {target_tbl} WHERE id = {q_id};"
                            else:
                                # Range or Filter Query
                                if "user" in target_tbl:
                                    executed_sql = f"SELECT id, name, status FROM {target_tbl} WHERE age > {random.randint(20, 50)} LIMIT 10;"
                                elif "order" in target_tbl:
                                    executed_sql = f"SELECT * FROM {target_tbl} WHERE status = 'COMPLETED' LIMIT 5;"
                                else:
                                    executed_sql = f"SELECT * FROM {target_tbl} WHERE cpu > 50.0 LIMIT 10;"
                            res = self.db.execute_query(executed_sql, target_db)
                            q_target = f"{target_db}.{target_tbl}"
                        elif dice < 0.90:
                            # UPDATE
                            op_type = "UPDATE"
                            existing = row_ids.get(tbl_key, [])
                            if existing:
                                q_id = random.choice(existing)
                                if "user" in target_tbl:
                                    executed_sql = f"UPDATE {target_tbl} SET age = {random.randint(20, 70)} WHERE id = {q_id};"
                                elif "order" in target_tbl:
                                    executed_sql = f"UPDATE {target_tbl} SET status = 'DELIVERED' WHERE id = {q_id};"
                                else:
                                    executed_sql = f"UPDATE {target_tbl} SET cpu = 88.8 WHERE id = {q_id};"
                                res = self.db.execute_query(executed_sql, target_db)
                            else:
                                op_type = "SEARCH"
                                executed_sql = f"SELECT * FROM {target_tbl} LIMIT 5;"
                                res = self.db.execute_query(executed_sql, target_db)
                            q_target = f"{target_db}.{target_tbl}"
                        else:
                            # DELETE
                            op_type = "DELETE"
                            existing = row_ids.get(tbl_key, [])
                            if existing and len(existing) > 5:
                                q_id = existing.pop(random.randrange(len(existing)))
                                executed_sql = f"DELETE FROM {target_tbl} WHERE id = {q_id};"
                                res = self.db.execute_query(executed_sql, target_db)
                            else:
                                op_type = "SEARCH"
                                executed_sql = f"SELECT * FROM {target_tbl} LIMIT 5;"
                                res = self.db.execute_query(executed_sql, target_db)
                            q_target = f"{target_db}.{target_tbl}"

                    # Record Telemetry
                    lat = res.get("latency_ms", 1.0)
                    is_ok = res.get("status") == "success"
                    ops_done += 1
                    if is_ok:
                        passed_ops += 1
                    else:
                        failed_ops += 1
                        failed_events.append({
                            "type": op_type,
                            "target": q_target,
                            "query": executed_sql,
                            "error": res.get("error") or "Unknown execution error"
                        })

                    if op_type in op_counts:
                        op_counts[op_type] += 1
                    latencies.append(lat)

                    now_str = time.strftime("%H:%M:%S")
                    recent_events.append({
                        "time": now_str,
                        "type": op_type,
                        "target": q_target,
                        "success": is_ok,
                        "lat": lat
                    })
                    if len(recent_events) > 20:
                        recent_events.pop(0)

                    # Update Live Dashboard
                    live.update(make_dashboard())
                    # Small delay to keep responsiveness balanced
                    time.sleep(0.01)

        except (KeyboardInterrupt, SystemExit):
            console.print("\n[yellow]Benchmark interrupted by user. Generating executive report...[/yellow]")

        # Final Report
        elapsed_total = max(0.001, time.time() - start_time)
        qps_final = ops_done / elapsed_total
        avg_lat = sum(latencies) / len(latencies) if latencies else 0.0
        min_lat = min(latencies) if latencies else 0.0
        max_lat = max(latencies) if latencies else 0.0
        pass_pct = (passed_ops / ops_done) * 100.0 if ops_done > 0 else 0.0

        console.print()
        console.print(Rule("[bold green]PINGDART BENCHMARK SCORECARD[/bold green]"))

        summary_table = Table(box=None, padding=(0, 2))
        summary_table.add_column("Benchmark Metric", style="bold cyan")
        summary_table.add_column("Measured Value", style="bold white")

        summary_table.add_row("Total Operations Completed", f"{ops_done:,}")
        summary_table.add_row("Total Elapsed Time", f"{elapsed_total:.2f} seconds")
        summary_table.add_row("Overall Throughput (QPS)", f"[bold green]{qps_final:.1f} ops/sec[/bold green]")
        summary_table.add_row("Operations Succeeded", f"[green]{passed_ops:,} ({pass_pct:.2f}%)[/green]")
        summary_table.add_row("Operations Failed", f"[red]{failed_ops:,}[/red]" if failed_ops > 0 else "[green]0[/green]")
        summary_table.add_row("Latency Range (Min / Avg / Max)", f"{min_lat:.2f} ms / {avg_lat:.2f} ms / {max_lat:.2f} ms")
        summary_table.add_row("Databases Managed", f"{len(dbs):,} DBs")
        summary_table.add_row("Tables Under Load", f"{sum(len(t) for t in tables.values()):,} tables")

        score = "A+" if pass_pct >= 98 else ("A" if pass_pct >= 90 else "B")
        summary_table.add_row("Stability & Performance Grade", f"[bold green]{score}[/bold green]")

        console.print(Panel(summary_table, border_style="green" if failed_ops == 0 else "yellow", title="[bold white]Executive Performance Summary[/bold white]"))

        if failed_events:
            console.print()
            diag_table = Table(title="[bold red]Failure Diagnostics & Root Cause Analysis[/bold red]", box=None, padding=(0, 1))
            diag_table.add_column("Op Type", style="bold yellow", width=12)
            diag_table.add_column("Target", style="cyan", width=20)
            diag_table.add_column("Query Executed", style="white")
            diag_table.add_column("Database Error", style="bold red")

            # Show unique failures
            seen_errs = set()
            for fe in failed_events:
                k = (fe["type"], fe["error"])
                if k not in seen_errs:
                    seen_errs.add(k)
                    q_disp = fe["query"][:45] + "..." if len(fe["query"]) > 45 else fe["query"]
                    e_disp = fe["error"][:50] + "..." if len(fe["error"]) > 50 else fe["error"]
                    diag_table.add_row(fe["type"], fe["target"], q_disp, e_disp)
                    if len(seen_errs) >= 8:
                        break

            console.print(Panel(diag_table, border_style="red"))
        console.print()

    def print_final_summary(self):
        if not self.history:
            return

        total = len(self.history)
        passed = sum(1 for h in self.history if h["passed"])
        failed = total - passed
        pass_rate = (passed / total) * 100.0 if total > 0 else 0
        latencies = [h["latency_ms"] for h in self.history]
        avg_lat = sum(latencies) / len(latencies) if latencies else 0.0
        min_lat = min(latencies) if latencies else 0.0
        max_lat = max(latencies) if latencies else 0.0

        console.print()
        console.print(Rule("[bold green]TEST RUN EXECUTIVE SUMMARY[/bold green]"))

        summary_table = Table(title="[bold white]Overall Database Test Metrics[/bold white]", box=None, padding=(0, 2))
        summary_table.add_column("Metric", style="bold cyan")
        summary_table.add_column("Value", style="bold white")

        summary_table.add_row("Total Tests Run", str(total))
        summary_table.add_row("Tests Passed", f"[green]{passed}[/green]")
        summary_table.add_row("Tests Failed", f"[red]{failed}[/red]" if failed > 0 else "[green]0[/green]")
        summary_table.add_row("Success Rate", f"[{'green' if pass_rate >= 80 else 'red'}]{pass_rate:.1f}%[/]")
        summary_table.add_row("Latency (Min / Avg / Max)", f"{min_lat:.2f} ms / {avg_lat:.2f} ms / {max_lat:.2f} ms")

        score = "A+" if pass_rate >= 95 else ("A" if pass_rate >= 90 else ("B" if pass_rate >= 75 else "C"))
        summary_table.add_row("Database Health Grade", f"[bold green]{score}[/bold green]" if score.startswith("A") else f"[bold yellow]{score}[/bold yellow]")

        console.print(Panel(summary_table, border_style="green" if failed == 0 else "yellow"))
        console.print()

    # =========================================================================
    # Phase 14: 12-Layer Autonomous Validation & Chaos Engine Runners
    # =========================================================================

    def run_chaos_battery(self):
        """Run the comprehensive 12-layer Phase 14 Autonomous Validation Battery."""
        self.chaos_orchestrator.run_full_suite()

    def run_fuzzer(self, count: int = 150):
        """Layer 1: AI SQL Fuzzer with Malformed & Boundary Queries."""
        fuzzer = SQLFuzzer(self.db, self.ollama, self.learner)
        fuzzer.run(count=count)

    def run_simulator(self, num_users: int = 12, max_workers: int = 6):
        """Layer 2: Concurrent Multi-User Behavior Simulator."""
        sim = UserBehaviorSimulator(self.db, self.learner)
        sim.run(num_users=num_users, max_workers=max_workers)

    def run_rbac_attacker(self):
        """Layer 3: Tenant Isolation & RBAC Penetration Attack Engine."""
        rbac = RBACAttackEngine(self.db, self.learner)
        rbac.run()

    def run_destroyer(self):
        """Layer 4: Database Destroyer & Concurrency Crash Chaos."""
        destroyer = DatabaseDestroyer(self.db, self.learner)
        destroyer.run()

    def run_torture(self):
        """Layer 5: Malformed JSON, CSV & Protobuf Import/Export Torture."""
        torture = ImportExportTorture(self.db, self.learner)
        torture.run()

    def run_red_team(self):
        """Layer 7: Security Red Team AI Penetration Tester."""
        red_team = SecurityRedTeam(self.db, self.learner)
        red_team.run()

    def run_explorer(self, steps: int = 10):
        """Layer 8: Autonomous Explorer Blackbox Hacker Agent."""
        explorer = AutonomousExplorer(self.db, self.ollama, self.learner)
        explorer.run(steps=steps)

    def run_state_validator(self, db_name: str = "bench_scale_0"):
        """Layer 9: State Invariant Silent Corruption Auditor."""
        validator = StateInvariantValidator(self.db, self.learner)
        validator.run(db_name=db_name)

    def run_resource_auditor(self, ops: int = 200):
        """Layer 10: RAM Leak, Thread & OS Handle Auditor."""
        auditor = ResourceAuditor(self.db, self.learner)
        auditor.run(ops=ops)

    def run_monkey_testing(self, duration_sec: int = 10):
        """Layer 11: Long-Duration Monkey Tester with Latency Drift Tracking."""
        monkey = MonkeyTester(self.db, self.learner)
        monkey.run(duration_sec=duration_sec)

    def run_dba_auditor(self, db_name: str = "bench_scale_0"):
        """Layer 12: AI DBA Schema & Query Optimizer."""
        dba = AIDBAAuditor(self.db, self.ollama)
        dba.run(db_name=db_name)

    def show_bug_ledger(self):
        """Layer 6: Self-Learning Bug Ledger & Regression Tests Summary."""
        self.learner.print_summary()

    # =========================================================================
    # Phase 14B: 24/7 Continuous Autonomous Validation & Learning Engine
    # =========================================================================

    def run_continuous_learning(self, max_cycles: Optional[int] = None, cycle_delay: float = 0.2):
        """Phase 14B: 24/7 Continuous Autonomous Validation & Learning Engine."""
        daemon = ContinuousDaemon(self.db, self.ollama)
        daemon.run(max_cycles=max_cycles, cycle_delay=cycle_delay)

    def show_daily_scorecard(self):
        """Show and refresh the latest daily health report."""
        learner = ContinuousLearningEngine(self.db, self.ollama)
        report = learner.generate_daily_scorecard()
        console.print(Panel(Markdown(report), title="[bold white]PingDart Daily Health Report[/bold white]", border_style="green"))

    def show_discovered_patterns(self):
        """Show discovered attack & failure patterns."""
        learner = ContinuousLearningEngine(self.db, self.ollama)
        if not learner.patterns:
            console.print("[green]No failure patterns currently discovered. All tests passing![/green]")
            return
        tbl = Table(title="[bold yellow]Discovered Attack & Failure Patterns[/bold yellow]", box=None, padding=(0, 1))
        tbl.add_column("Discovered Pattern", style="bold white")
        tbl.add_column("Layer", style="cyan")
        tbl.add_column("Times Seen", justify="center", style="yellow")
        tbl.add_column("Severity", justify="center", style="red")
        for p in learner.patterns:
            tbl.add_row(p.get("pattern", ""), p.get("layer", ""), str(p.get("times_seen", 1)), str(p.get("severity", "")))
        console.print(Panel(tbl, border_style="yellow"))

    # =========================================================================
    # Phases 14C-14F & Phase 15: Coverage, Endurance, Plan, RootCause & Cluster
    # =========================================================================

    def run_coverage_intel(self, hunt: bool = True):
        """Phase 14C: Coverage Intelligence Engine."""
        engine = CoverageIntelligenceEngine(self.db)
        if hunt:
            engine.hunt_blindspots()
        engine.print_coverage_matrix()

    def run_endurance_daemon(self, snapshots: int = 1):
        """Phase 14D: Long-Term Endurance Telemetry & Resource Trends."""
        engine = EnduranceDaemon(self.db)
        engine.print_endurance_report()

    def run_plan_regression(self):
        """Phase 14E: Query Plan & Performance Regression Detection."""
        engine = QueryPlanRegressionDetector(self.db)
        engine.print_plan_audit_report()

    def run_root_cause_analyzer(self):
        """Phase 14F: AI Root Cause Analyzer & Autonomous Repair Patches."""
        engine = AIRootCauseAnalyzer(self.ollama)
        engine.run_all()

    def run_cluster_chaos(self):
        """Phase 15: Distributed Cluster Chaos & Raft Consensus Testing."""
        engine = ClusterChaosEngine(self.db)
        engine.run_chaos_battery()


def main():
    parser = argparse.ArgumentParser(description="PingDart AI Database Tester using Ollama")
    parser.add_argument("--mode", choices=[
        "suite", "fuzz", "interactive", "prompt", "benchmark",
        "chaos", "simulator", "rbac", "destroyer", "torture",
        "redteam", "explorer", "state-audit", "resource-audit",
        "monkey", "dba", "bugs",
        "continuous-learning", "daemon", "247", "scorecard", "patterns",
        "coverage", "endurance", "plan-regression", "root-cause", "cluster-chaos"
    ], default="suite",
    help="Execution mode (continuous-learning: 24/7 autonomous learning daemon, coverage: endpoint coverage, cluster-chaos: raft fault injection)")
    parser.add_argument("--model", type=str, default=None, help="Ollama model to use (e.g. deepseek-coder:latest, qwen2.5-coder:latest)")
    parser.add_argument("--ollama-url", type=str, default=OLLAMA_DEFAULT_URL, help="Ollama base API URL")
    parser.add_argument("--db-url", type=str, default=PINGDART_DEFAULT_URL, help="PingDart Core HTTP gateway URL")
    parser.add_argument("--prompt", type=str, default=None, help="Custom test prompt when mode is 'prompt'")
    parser.add_argument("--count", type=int, default=1000, help="Number of operations for benchmark or fuzzer mode (default: 1000)")
    parser.add_argument("--continuous", action="store_true", default=False, help="Run continuous infinite benchmark until stopped")
    parser.add_argument("--max-cycles", type=int, default=None, help="Maximum cycles for continuous learning (default: infinite)")
    parser.add_argument("--cycle-delay", type=float, default=0.2, help="Delay in seconds between continuous learning cycles")
    parser.add_argument("--ai-interval", type=int, default=50, help="Frequency of AI chaos mutations during benchmark")
    parser.add_argument("--auto-start", action="store_true", default=True, help="Auto start DB binary if not running")

    args = parser.parse_args()

    ollama = OllamaClient(base_url=args.ollama_url, model=args.model)
    db = PingDartClient(base_url=args.db_url)

    # 1. Connect to Ollama
    ollama_ok, available_models = ollama.check_connection()
    if not ollama_ok:
        console.print(f"[bold red]Error: Could not connect to Ollama at {args.ollama_url}.[/bold red]")
        console.print("[yellow]Make sure Ollama is running (`ollama serve` or Ollama app).[/yellow]")
        sys.exit(1)

    # 2. Connect to PingDart DB
    if not db.is_running():
        console.print(f"[yellow]PingDart DB is not responding on {args.db_url}.[/yellow]")
        if args.auto_start:
            if not db.auto_start_db():
                console.print("[red]Could not auto-start PingDartDB. Please run `cargo run --bin pingdart-core` first.[/red]")
                sys.exit(1)
        else:
            console.print("[red]Please start PingDartDB or specify --auto-start.[/red]")
            sys.exit(1)

    # 3. Authenticate with PingDart DB
    if not db.login():
        console.print("[bold red]Failed to authenticate with PingDart DB. Check admin/admin123 credentials.[/bold red]")
        sys.exit(1)

    agent = AITesterAgent(ollama, db)
    agent.print_header(available_models)

    if args.mode == "suite":
        agent.run_automated_suite()
    elif args.mode == "fuzz":
        agent.run_fuzzer(count=args.count if args.count != 1000 else 150)
    elif args.mode == "benchmark":
        agent.run_continuous_benchmark(count=args.count, continuous=args.continuous, ai_interval=args.ai_interval)
    elif args.mode == "chaos":
        agent.run_chaos_battery()
    elif args.mode in ["continuous-learning", "daemon", "247"]:
        agent.run_continuous_learning(max_cycles=args.max_cycles, cycle_delay=args.cycle_delay)
    elif args.mode == "scorecard":
        agent.show_daily_scorecard()
    elif args.mode == "patterns":
        agent.show_discovered_patterns()
    elif args.mode == "coverage":
        agent.run_coverage_intel()
    elif args.mode == "endurance":
        agent.run_endurance_daemon()
    elif args.mode in ["plan-regression", "plan"]:
        agent.run_plan_regression()
    elif args.mode in ["root-cause", "rootcause", "patch"]:
        agent.run_root_cause_analyzer()
    elif args.mode in ["cluster-chaos", "cluster"]:
        agent.run_cluster_chaos()
    elif args.mode == "simulator":
        agent.run_simulator()
    elif args.mode == "rbac":
        agent.run_rbac_attacker()
    elif args.mode == "destroyer":
        agent.run_destroyer()
    elif args.mode == "torture":
        agent.run_torture()
    elif args.mode == "redteam":
        agent.run_red_team()
    elif args.mode == "explorer":
        agent.run_explorer()
    elif args.mode == "state-audit":
        agent.run_state_validator()
    elif args.mode == "resource-audit":
        agent.run_resource_auditor()
    elif args.mode == "monkey":

        agent.run_monkey_testing()
    elif args.mode == "dba":
        agent.run_dba_auditor()
    elif args.mode == "bugs":
        agent.show_bug_ledger()
    elif args.mode == "interactive":
        agent.run_interactive_repl()
    elif args.mode == "prompt":
        if not args.prompt:
            console.print("[red]Please supply --prompt 'your test instruction'[/red]")
            sys.exit(1)
        console.print(f"[bold cyan]Running AI test for:[/bold cyan] {args.prompt}")
        agent.run_single_test({"category": "CUSTOM", "description": args.prompt, "query": args.prompt, "expected_success": True}, 1, 1)
        agent.print_final_summary()




if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        console.print("\n[yellow]Operation aborted by user (Ctrl+C).[/yellow]")
        sys.exit(0)

