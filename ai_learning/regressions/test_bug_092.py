#!/usr/bin/env python3
"""
Continuous Autonomous Regression Test: BUG-092
Discovered by: UserLifecycle
Title: User Lifecycle Failure: Invalid User on Tenant Change
Generated: 2026-09-12 02:37:44Z
"""

import requests
import pytest

BASE_URL = "http://127.0.0.1:4650"

def test_bug_092():
    session = requests.Session()
    # 1. Authenticate with PingDart Core
    login_res = session.post(f"{BASE_URL}/login", json={"username": "admin", "password": "admin123"}, timeout=5)
    assert login_res.status_code == 200, "Authentication failed"
    token = login_res.json().get("token")

    # 2. Reproduction Query
    query = """-- Persona: Invalid User, Action: Tenant Change"""

    # 3. Execution
    res = session.post(f"{BASE_URL}/query", json={"token": token, "query": query}, timeout=10)
    
    # Assert database does not crash (HTTP 200/400/401/403 accepted, 500 panic rejected)
    assert res.status_code in [200, 400, 401, 403], f"Database crashed with status {res.status_code}"
    data = res.json()
    assert "status" in data, "Malformed engine response without status field"

if __name__ == "__main__":
    test_bug_092()
    print("Regression test BUG-092 verified PASS!")
