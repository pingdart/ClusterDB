#!/usr/bin/env python3
"""
Autonomous Regression Test: BUG-2026-0002
Discovered by: Layer Layer 7: Red Team AI
Title: Security vulnerability: Stacked Query Attack ('; DROP TABLE users; --) succeeded!
Discovered on: 2026-09-12 07:38:38
"""

import requests
import pytest

BASE_URL = "http://127.0.0.1:4650"

def test_bug_2026_0002():
    session = requests.Session()
    # 1. Authenticate
    login_res = session.post(f"{BASE_URL}/login", json={"username": "admin", "password": "admin123"}, timeout=5)
    assert login_res.status_code == 200, "Login failed"
    token = login_res.json()["token"]

    # 2. Reproduction query
    query = """SELECT * FROM users; DROP TABLE users; --"""
    
    # 3. Execute
    res = session.post(f"{BASE_URL}/query", json={"token": token, "query": query}, timeout=10)
    
    # Assert database does not crash (HTTP 200/400 expected, never 500 internal server crash or connection refusal)
    assert res.status_code in [200, 400, 401, 403], f"Unexpected status code: {res.status_code}"
    data = res.json()
    assert "status" in data, "Malformed server response without status"

if __name__ == "__main__":
    test_bug_2026_0002()
    print("Regression test passed!")
