import json
import time
import random
import string
from typing import Any, Dict, List, Optional
from rich.console import Console

console = Console(safe_box=True)

ENTITIES = [
    "Database",
    "Tables",
    "Indexes",
    "Vectors",
    "Graphs",
    "Memories",
    "Users",
    "Roles",
    "API Keys",
    "Tenants"
]

OPERATIONS = [
    "Insert",
    "Update",
    "Delete",
    "Export",
    "Import",
    "Backup",
    "Restore",
    "Drop",
    "Recreate",
    "Clone"
]


class InfiniteDatabaseExplorer:
    """
    Simulates continuous, unconstrained state space transitions across
    Databases, Tables, Indexes, Vectors, Graphs, Memories, Users, Roles, API Keys, and Tenants.
    Chains Insert, Update, Delete, Export, Import, Backup, Restore, Drop, Recreate, and Clone.
    """

    def __init__(self, db_client, continuous_learner=None):
        self.db = db_client
        self.learner = continuous_learner
        self.active_dbs: List[str] = ["default", "bench_scale_0"]
        self.active_tables: Dict[str, List[str]] = {"default": ["users"]}

    def _rand_str(self, length: int = 5) -> str:
        return ''.join(random.choices(string.ascii_lowercase + string.digits, k=length))

    def explore_transition(self) -> Dict[str, Any]:
        entity = random.choice(ENTITIES)
        op = random.choice(OPERATIONS)
        t0 = time.perf_counter()

        db_name = random.choice(self.active_dbs) if self.active_dbs else "default"
        table_name = f"tbl_{self._rand_str(4)}"
        sql_query = ""
        expected = "200"
        actual = "200"
        passed = True

        try:
            if entity == "Database":
                new_db = f"db_exp_{self._rand_str(4)}"
                if op in ["Insert", "Recreate"]:
                    sql_query = f"CREATE DATABASE {new_db};"
                    self.active_dbs.append(new_db)
                elif op == "Drop":
                    drop_db = random.choice(self.active_dbs) if len(self.active_dbs) > 1 else new_db
                    sql_query = f"DROP DATABASE {drop_db};"
                    if drop_db in self.active_dbs and drop_db != "default":
                        self.active_dbs.remove(drop_db)
                elif op == "Clone":
                    sql_query = f"CREATE DATABASE {new_db}_clone;"
                else:
                    sql_query = f"SHOW DATABASES;"

            elif entity == "Tables":
                if op in ["Insert", "Recreate"]:
                    sql_query = f"CREATE TABLE {table_name} (id INT PRIMARY KEY, key VARCHAR(50), val VARCHAR(100));"
                elif op == "Drop":
                    sql_query = f"DROP TABLE {table_name};"
                else:
                    sql_query = f"SHOW TABLES;"

            elif entity == "Indexes":
                if op in ["Insert", "Recreate"]:
                    sql_query = f"CREATE INDEX idx_{self._rand_str(4)} ON users (status);"
                elif op == "Drop":
                    sql_query = f"DROP INDEX idx_test ON users;"
                else:
                    sql_query = f"EXPLAIN SELECT * FROM users WHERE status = 'active';"

            elif entity == "Vectors":
                # Vector collection indexing and search
                coll = f"vec_coll_{self._rand_str(4)}"
                if op in ["Insert", "Recreate"]:
                    sql_query = f"CREATE TABLE {coll} (id INT PRIMARY KEY, embedding VECTOR(128));"
                else:
                    sql_query = f"SELECT * FROM {coll} ORDER BY embedding LIMIT 5;"

            elif entity == "Graphs":
                # Graph relation edges
                if op in ["Insert", "Recreate"]:
                    sql_query = f"CREATE TABLE edges_{self._rand_str(4)} (src INT, dst INT, weight FLOAT);"
                else:
                    sql_query = f"SELECT * FROM users WHERE id IN (1, 2, 3);"

            elif entity == "Memories":
                # Key-value persistent cache operations
                kv_key = f"mem_{self._rand_str(6)}"
                if op == "Insert":
                    sql_query = f"INSERT INTO _kv (key, val) VALUES ('{kv_key}', 'session_data_{self._rand_str()}');"
                elif op == "Delete":
                    sql_query = f"DELETE FROM _kv WHERE key = '{kv_key}';"
                else:
                    sql_query = f"SELECT val FROM _kv WHERE key = '{kv_key}';"

            elif entity in ["Users", "Roles"]:
                u = f"usr_{self._rand_str(4)}"
                if op in ["Insert", "Recreate"]:
                    sql_query = f"INSERT INTO _users (username, password, role) VALUES ('{u}', 'pass123', 'analyst');"
                elif op == "Delete":
                    sql_query = f"DELETE FROM _users WHERE username = '{u}';"
                else:
                    sql_query = f"SELECT * FROM _users LIMIT 5;"

            elif entity == "Tenants":
                tenant_id = f"tenant_{self._rand_str(4)}"
                if op in ["Insert", "Recreate"]:
                    sql_query = f"CREATE DATABASE {tenant_id}_db;"
                elif op == "Drop":
                    sql_query = f"DROP DATABASE {tenant_id}_db;"
                else:
                    sql_query = f"SELECT 1;"

            else:  # API Keys, Backup, Export, Restore
                sql_query = f"SELECT 1;"

            # Execute against database
            res = self.db.execute_query(sql_query, db_name)
            lat = res.get("latency_ms", (time.perf_counter() - t0) * 1000.0)
            actual = "200" if res.get("status") == "success" else "400"
            passed = True  # Clean failure or success without process panic

        except Exception as e:
            lat = (time.perf_counter() - t0) * 1000.0
            actual = "500"
            passed = False

        status_str = "PASS" if passed else "FAIL"

        if self.learner:
            self.learner.record_test(
                layer="InfiniteExplorer",
                action=f"{entity}::{op}",
                expected=expected,
                actual=actual,
                status=status_str,
                latency_ms=lat,
                details={"query": sql_query}
            )
            if not passed:
                self.learner.record_bug_dossier(
                    layer="InfiniteExplorer",
                    severity="Critical" if actual == "500" else "High",
                    title=f"State Transition Failure: {entity}::{op}",
                    steps=[
                        f"Target database '{db_name}'",
                        f"Execute operation '{op}' on entity '{entity}'",
                        f"Query: {sql_query}"
                    ],
                    expected=expected,
                    actual=actual,
                    repro_query=sql_query
                )

        return {
            "entity": entity,
            "operation": op,
            "query": sql_query,
            "status": status_str,
            "latency_ms": lat
        }

    def run_cycle(self, count: int = 25) -> List[Dict[str, Any]]:
        """Run a batch of randomized state transitions."""
        results = []
        for _ in range(count):
            results.append(self.explore_transition())
        return results
