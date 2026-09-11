import os
import sys
import time
import json
import shutil
import subprocess
import requests
import base64
import hmac
import hashlib

SERVER_PORT = 4780
BASE_URL = f"http://127.0.0.1:{SERVER_PORT}"
DATA_DIR = "data_security_test"

def clean_data():
    if os.path.exists(DATA_DIR):
        shutil.rmtree(DATA_DIR)

def start_server():
    clean_data()
    env = os.environ.copy()
    env["RUST_LOG"] = "warn"
    proc = subprocess.Popen(["./target/debug/pingdart-core", "--port", str(SERVER_PORT), "--data-dir", DATA_DIR], env=env)
    
    for _ in range(30):
        try:
            r = requests.get(f"{BASE_URL}/", timeout=1)
            if r.status_code == 200:
                print(f"✅ PingDartDB Server started and responsive on port {SERVER_PORT}")
                return proc
        except Exception:
            time.sleep(0.3)
    
    proc.kill()
    raise RuntimeError("Failed to start PingDartDB server")

def b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).decode('utf-8').rstrip('=')

def generate_test_jwt(sub: str, roles: list, tenant_id: str = None, exp_offset: int = 3600, secret: str = "pingdart_enterprise_jwt_default_secret_2026") -> str:
    header = {"alg": "HS256", "typ": "JWT"}
    now = int(time.time())
    payload = {
        "sub": sub,
        "iss": "pingdart-internal-idp",
        "roles": roles,
        "tenant_id": tenant_id,
        "exp": now + exp_offset
    }
    
    h_b64 = b64url(json.dumps(header, separators=(',', ':')).encode('utf-8'))
    p_b64 = b64url(json.dumps(payload, separators=(',', ':')).encode('utf-8'))
    signing_input = f"{h_b64}.{p_b64}"
    
    sig = hmac.new(secret.encode('utf-8'), signing_input.encode('utf-8'), hashlib.sha256).digest()
    sig_b64 = b64url(sig)
    
    return f"{signing_input}.{sig_b64}"

def run_tests():
    proc = start_server()
    try:
        print("\n--- Test 1: Capability Matrix & Status Verification ---")
        status_res = requests.get(f"{BASE_URL}/").json()
        capabilities = status_res.get("capabilities", [])
        assert "Granular RBAC & Service Accounts" in capabilities
        assert "Scoped API Key Management" in capabilities
        assert "OIDC / JWT Bearer Authentication" in capabilities
        assert "AES-256-GCM Envelope Encryption" in capabilities
        print("✅ Status capabilities include full Phase 9 security suite.")

        print("\n--- Test 2: Admin Session Login ---")
        login_res = requests.post(f"{BASE_URL}/login", json={
            "username": "admin",
            "password": "admin123"
        }).json()
        assert login_res["status"] == "success", f"Login failed: {login_res}"
        admin_token = login_res["token"]
        print("✅ SuperAdmin logged in successfully.")

        print("\n--- Test 3: Preloaded RBAC Roles & Custom Role Creation ---")
        roles_res = requests.get(f"{BASE_URL}/security/role/list").json()
        assert roles_res["status"] == "success"
        roles = json.loads(roles_res["result"])
        role_names = [r["name"] for r in roles]
        assert "SuperAdmin" in role_names
        assert "DataScientist" in role_names
        assert "AppDeveloper" in role_names
        assert "ReadOnly" in role_names
        print(f"✅ Preloaded roles verified: {role_names}")

        # Create custom scoped role
        create_role_res = requests.post(f"{BASE_URL}/security/role/create", json={
            "token": admin_token,
            "name": "TenantAuditor",
            "description": "Read-only auditor for specific tenant",
            "permissions": ["AuditRead", "TableRead"],
            "scope": "tenant:org_alpha"
        }).json()
        assert create_role_res["status"] == "success", f"Create role failed: {create_role_res}"
        print("✅ Custom scoped role 'TenantAuditor' registered successfully.")

        print("\n--- Test 4: Scoped API Key Creation ---")
        api_key_res = requests.post(f"{BASE_URL}/security/apikey/create", json={
            "token": admin_token,
            "name": "ai-analyst-key",
            "roles": ["DataScientist"],
            "tenant_id": "org_alpha",
            "ttl_secs": 7200
        }).json()
        assert api_key_res["status"] == "success", f"Failed to create API key: {api_key_res}"
        key_data = json.loads(api_key_res["result"])
        raw_api_key = key_data["api_key"]
        key_id = key_data["key_id"]
        assert raw_api_key.startswith("pd_live_")
        print(f"✅ Generated API key: {key_data['prefix']} (ID: {key_id})")

        # List API keys
        list_keys_res = requests.post(f"{BASE_URL}/security/apikey/list", json={"token": admin_token}).json()
        assert list_keys_res["status"] == "success"
        keys_list = json.loads(list_keys_res["result"])
        assert any(k["key_id"] == key_id for k in keys_list)
        print("✅ API key management listing verified.")

        print("\n--- Test 5: Seed Relational Data with SuperAdmin ---")
        res = requests.post(f"{BASE_URL}/query", json={
            "query": "CREATE TABLE medical_records (id INT, patient TEXT, diagnosis TEXT);",
            "token": admin_token
        }).json()
        assert res["status"] == "success", f"CREATE TABLE failed: {res}"

        res = requests.post(f"{BASE_URL}/query", json={
            "query": "INSERT INTO medical_records VALUES (101, 'Patient A', 'Cardiomyopathy');",
            "token": admin_token
        }).json()
        assert res["status"] == "success", f"INSERT failed: {res}"
        print("✅ Table created and record inserted by SuperAdmin.")

        print("\n--- Test 6: Querying with Scoped API Key & RBAC Enforcement ---")
        # 6a. Read query with DataScientist key -> Allowed
        read_res = requests.post(f"{BASE_URL}/query", json={
            "query": "SELECT * FROM medical_records;",
            "token": raw_api_key
        }).json()
        assert read_res["status"] == "success", f"API Key SELECT failed: {read_res}"
        rows = json.loads(read_res["result"])
        assert len(rows) == 1
        assert rows[0]["patient"] == "Patient A"
        print("✅ API Key successfully authenticated and executed SELECT.")

        # 6b. Write query with DataScientist key -> Must be rejected by RBAC!
        write_res = requests.post(f"{BASE_URL}/query", json={
            "query": "INSERT INTO medical_records VALUES (102, 'Patient B', 'Arrhythmia');",
            "token": raw_api_key
        }).json()
        assert write_res["status"] == "error", f"Write should have been denied but got: {write_res}"
        assert "TableWrite" in write_res.get("error", "") or "denied" in write_res.get("error", "")
        print(f"✅ RBAC enforcement blocked unprivileged write: {write_res['error']}")

        print("\n--- Test 7: API Key Instant Revocation ---")
        revoke_res = requests.post(f"{BASE_URL}/security/apikey/revoke", json={
            "token": admin_token,
            "key_id": key_id
        }).json()
        assert revoke_res["status"] == "success"
        print("✅ API key revoked.")

        # Immediate verification that revoked key is rejected
        rejected_res = requests.post(f"{BASE_URL}/query", json={
            "query": "SELECT * FROM medical_records;",
            "token": raw_api_key
        }).json()
        assert rejected_res["status"] == "error"
        assert "revoked" in rejected_res["error"].lower() or "failed" in rejected_res["error"].lower()
        print(f"✅ Revoked API key access immediately rejected: {rejected_res['error']}")

        print("\n--- Test 8: OIDC / JWT Bearer Authentication ---")
        # Valid JWT with SuperAdmin role
        jwt_token = generate_test_jwt("service_agent_k8s", ["SuperAdmin"])
        jwt_res = requests.post(f"{BASE_URL}/query", json={
            "query": "SELECT * FROM medical_records;",
            "token": jwt_token
        }).json()
        assert jwt_res["status"] == "success", f"JWT query failed: {jwt_res}"
        print("✅ OIDC / JWT bearer token authenticated and query executed.")

        # Tampered JWT -> Must fail
        tampered_jwt = jwt_token[:-4] + "ffff"
        bad_jwt_res = requests.post(f"{BASE_URL}/query", json={
            "query": "SELECT * FROM medical_records;",
            "token": tampered_jwt
        }).json()
        assert bad_jwt_res["status"] == "error"
        print(f"✅ Tampered JWT signature rejected: {bad_jwt_res['error']}")

        # Expired JWT -> Must fail
        expired_jwt = generate_test_jwt("expired_agent", ["SuperAdmin"], exp_offset=-100)
        expired_res = requests.post(f"{BASE_URL}/query", json={
            "query": "SELECT * FROM medical_records;",
            "token": expired_jwt
        }).json()
        assert expired_res["status"] == "error"
        assert "expired" in expired_res["error"].lower()
        print(f"✅ Expired JWT rejected: {expired_res['error']}")

        print("\n--- Test 9: AES-256-GCM Envelope Encryption at Rest ---")
        plaintext = "PingDartDB Confidential AI Memory & Embedding Weights 2026"
        encrypt_res = requests.post(f"{BASE_URL}/security/encrypt", json={
            "token": admin_token,
            "plaintext": plaintext
        }).json()
        assert encrypt_res["status"] == "success", f"Encrypt failed: {encrypt_res}"
        ciphertext_hex = encrypt_res["result"]
        assert len(ciphertext_hex) > len(plaintext) * 2  # Nonce + tag overhead
        print(f"✅ Plaintext encrypted into {len(ciphertext_hex)} hex characters.")

        decrypt_res = requests.post(f"{BASE_URL}/security/decrypt", json={
            "token": admin_token,
            "ciphertext_hex": ciphertext_hex
        }).json()
        assert decrypt_res["status"] == "success", f"Decrypt failed: {decrypt_res}"
        decrypted_text = decrypt_res["result"]
        assert decrypted_text == plaintext
        print(f"✅ Decrypted text matched plaintext exactly: '{decrypted_text}'")

        # Tampered ciphertext decryption -> Must fail authentication tag check
        tampered_hex = ciphertext_hex[:-2] + ("00" if ciphertext_hex[-2:] != "00" else "ff")
        bad_decrypt_res = requests.post(f"{BASE_URL}/security/decrypt", json={
            "token": admin_token,
            "ciphertext_hex": tampered_hex
        }).json()
        assert bad_decrypt_res["status"] == "error"
        print(f"✅ Tampered ciphertext authentication verification failed properly: {bad_decrypt_res['error']}")

        print("\n🎉 ALL PHASE 9 ENTERPRISE SECURITY TESTS PASSED PERFECTLY! 🎉\n")

    finally:
        proc.kill()
        proc.wait()
        clean_data()

if __name__ == "__main__":
    run_tests()
