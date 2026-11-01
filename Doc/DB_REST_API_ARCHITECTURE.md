# PingDart Cloud — Database REST API & Architecture Specification

**Version:** 1.0 (Aligned with PRD v1.0 & Rust Integration Spec v1.0)  
**Layer:** Data Plane / Database Engine & REST Gateway  
**Base URL:** `http://localhost:4650/v1` (or `https://api.pingdart.com/v1`)  

---

## 1. Modular File Structure

In accordance with `PingDart_Rust_Project_Structure_Integration_Spec.md` Section 3 & 5, the monolithic database server routes have been modularized into a dedicated, clean API layer:

```text
Database/src/
├── api/                                <--- Modular PRD DB API Engine
│   ├── mod.rs                          <--- Master API Router aggregator & state connector
│   ├── dto.rs                          <--- Request/Response models, FilterOp parser, pagination
│   ├── error.rs                        <--- PRD Section 23 Machine-readable Error models & ApiError
│   ├── data.rs                         <--- PRD Section 6.4 REST CRUD (/v1/data/:table) & rich queries
│   ├── schema.rs                       <--- PRD Section 6.2 Schema & Table DDL metadata endpoints
│   └── query.rs                        <--- PRD Section 6.5 SQL & document query execution
│
├── realtime/
│   └── mod.rs                          <--- WebSocket Hub & CDC streaming
├── storage/
│   └── mod.rs                          <--- Sled storage engine & transactions
├── exec.rs                             <--- Core query planner & executor
├── server.rs                           <--- HTTP server orchestrator & router mounting
├── lib.rs                              <--- Rust library exports (includes pub mod api;)
├── main.rs                             <--- CLI entrypoint & server bootstrap
│
├── sdk/
│   └── pingdart_realtime.dart          <--- Flutter & Dart SDK with PRD query/schema support
└── ui/
    └── realtime_studio.html            <--- Realtime Studio UI with live query & schema inspector
```

---

## 2. PRD Section 6.4 REST Data CRUD APIs

All data endpoints automatically inherit multi-tenant isolation via the `X-PingDart-Project` header or `?database=...` query parameter (defaulting to `"default"`).

### 2.1 List Records (Rich Filtering & Pagination)
- **Method / Path:** `GET /v1/data/:table`
- **Headers:** `X-PingDart-Project: <project_id>` (optional)

#### Query Capabilities:
| Query Parameter | Description | Example |
| :--- | :--- | :--- |
| `limit` | Max records to return (1 - 1000, default: 50) | `limit=20` |
| `offset` | Record offset index | `offset=40` |
| `page` | 1-indexed page calculation (`offset = (page - 1) * limit`) | `page=2` |
| `select` | Comma-separated field projection (always preserves `id`) | `select=id,user,status,price` |
| `order` / `sort` | Sort direction (`.asc`, `:asc`, `.desc`, `:desc`) | `order=created_at.desc` |
| `q` / `search` | Free-text search across all record attributes | `q=chinnu` |
| `<field>=<op>.<val>` | Rich PostgREST/PRD filter operator | `status=eq.active&price=gt.100` |

#### Supported Filter Operators:
- `eq.<val>`: Equals
- `neq.<val>`: Not equals
- `gt.<val>`: Greater than (numeric/string aware)
- `gte.<val>`: Greater than or equal to
- `lt.<val>`: Less than
- `lte.<val>`: Less than or equal to
- `like.<val>`: Wildcard pattern matching (e.g. `like.*vip*`)
- `ilike.<val>`: Case-insensitive wildcard pattern matching
- `in.(a,b,c)`: Set membership
- `is.null`: Field is null, empty or missing
- `not.null`: Field is present and non-null

#### Example Response:
```json
{
  "status": "success",
  "table": "subscriptions",
  "count": 2,
  "data": [
    {
      "id": "sub_101",
      "user_id": "usr_99",
      "status": "active",
      "plan": "pro",
      "price": 29.99
    },
    {
      "id": "sub_102",
      "user_id": "usr_100",
      "status": "active",
      "plan": "starter",
      "price": 9.99
    }
  ],
  "pagination": {
    "total": 42,
    "page": 1,
    "limit": 20,
    "offset": 0,
    "has_more": true
  }
}
```

---

### 2.2 Create Record (Single & Batch)
- **Method / Path:** `POST /v1/data/:table`
- **Headers:** `Idempotency-Key: <unique_key>` (optional)

#### Single Record Payload:
```json
{
  "user": "Alice",
  "email": "alice@example.com",
  "status": "active"
}
```
**Response (201 Created):**
```json
{
  "status": "success",
  "id": "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
  "table": "users",
  "record": {
    "id": "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
    "user": "Alice",
    "email": "alice@example.com",
    "status": "active"
  }
}
```

#### Batch Records Payload:
```json
[
  { "name": "Item A", "sku": "A1" },
  { "name": "Item B", "sku": "B2" }
]
```
**Response (201 Created):**
```json
{
  "status": "success",
  "table": "products",
  "count": 2,
  "ids": ["id_1", "id_2"],
  "records": [...]
}
```

---

### 2.3 Get Record by ID
- **Method / Path:** `GET /v1/data/:table/:id?select=field1,field2`
- **Response (200 OK):**
```json
{
  "status": "success",
  "table": "users",
  "data": {
    "id": "usr_123",
    "user": "Alice",
    "status": "active"
  }
}
```

---

### 2.4 Update Record (PATCH / PUT)
- **Method / Path:** `PATCH /v1/data/:table/:id` or `PUT /v1/data/:table/:id`
- **Payload:**
```json
{
  "status": "suspended",
  "tier": "enterprise"
}
```
**Response (200 OK):** Automatically increments version and emits CDC `UPDATE` event.
```json
{
  "status": "success",
  "id": "usr_123",
  "table": "users",
  "record": {
    "id": "usr_123",
    "status": "suspended",
    "tier": "enterprise"
  }
}
```

---

### 2.5 Delete Record
- **Method / Path:** `DELETE /v1/data/:table/:id`
- **Response (200 OK):** Emits CDC `DELETE` event.
```json
{
  "status": "success",
  "table": "users",
  "deleted_id": "usr_123"
}
```

---

## 3. PRD Section 6.2 Schema & Table Management APIs

| Method | Endpoint | Description |
| :--- | :--- | :--- |
| `GET` | `/v1/schema/tables` | List all tables, column definitions, and active row counts |
| `POST` | `/v1/schema/tables` | Create a table with defined columns, primary keys, and types |
| `GET` | `/v1/schema/tables/:table` | Inspect table schema and metadata |
| `DELETE` | `/v1/schema/tables/:table` | Drop table and delete data tree |
| `POST` | `/v1/schema/tables/:table/truncate` | Truncate all rows in table |

---

## 4. PRD Section 6.5 Query Execution API

- **Method / Path:** `POST /v1/query` or `POST /v1/data/query`
- **Payload:**
```json
{
  "query": "SELECT * FROM subscriptions WHERE status = 'active' ORDER BY expiry_date DESC LIMIT 5",
  "database": "default"
}
```
- **Response (200 OK):**
```json
{
  "status": "success",
  "database": "default",
  "result": [...]
}
```

---

## 5. PRD Section 23 Standard Error Envelope

All API errors return a consistent machine-readable error format:

```json
{
  "status": "error",
  "error": {
    "code": "RECORD_NOT_FOUND",
    "message": "Record 'sub_999' not found in table 'subscriptions'",
    "request_id": "req_a1b2c3d4"
  },
  "message": "Record 'sub_999' not found in table 'subscriptions'"
}
```

---

## 6. Dart & Flutter SDK Integration

Developers can interact with the updated APIs using `pingdart_realtime.dart`:

```dart
final client = PingDartClient(
  httpUrl: 'http://localhost:4650',
  database: 'default',
);

final subscriptions = client.collection('subscriptions');

// 1. Rich query with operators & pagination
final activeSubs = await subscriptions.list(
  filters: {'status': 'eq.active', 'price': 'gt.20'},
  select: 'id,plan,price',
  order: 'price.desc',
  limit: 10,
);

// 2. Real-time stream
subscriptions.onSnapshot().listen((event) {
  print('Realtime event: ${event['event']} on ${event['table']}');
});

// 3. Batch insert
await subscriptions.addBatch([
  {'plan': 'pro', 'price': 29.99},
  {'plan': 'enterprise', 'price': 99.99},
]);
```
