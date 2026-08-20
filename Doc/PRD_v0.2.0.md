# Product Requirements Document (PRD)
## PingDart-Core v0.2.0 – Intelligent Indexing & Query Engine

**Version:** 0.2.0
**Project:** PingDart-Core Database Engine
**Owner:** PingDart Team
**Priority:** Critical
**Estimated Duration:** 1 Sprint (7–14 Days)

### Vision
Transform PingDart-Core from a simple JSON document database into an intelligent database engine capable of querying millions or billions of documents efficiently through advanced indexing and query planning.

This release lays the architectural foundation for future enterprise features such as transactions, distributed storage, replication, and analytics.

---

### Current Problems
Current query execution:
`Read Collection -> Read Every Document -> Evaluate WHERE -> Return Result`
**Time Complexity:** O(n)

**Example:**
```sql
SELECT * FROM doctors WHERE speciality='Cardiologist';
```
*Current Engine:* 10 Million Documents -> 10 Million Comparisons -> Result
For large datasets, this becomes increasingly slow.

---

### Goal
Introduce an intelligent indexing subsystem that allows the engine to locate matching documents without scanning the entire collection.
**Target:** O(log n) or O(1) instead of O(n)

### Release Checklist
- [x] Secondary indexes implemented
- [x] Composite indexes implemented
- [x] Query planner functional
- [x] EXPLAIN command available
- [x] Background index builder
- [x] Index manager commands
- [x] Index statistics
- [x] Memory cache for indexes
- [x] Benchmark suite
- [x] API endpoints
- [x] SDK support
- [x] Unit and integration tests
- [x] Documentation complete

---

### Architecture
```text
                  SQL Parser
                       │
                       ▼
               Query Planner
                       │
        ┌──────────────┴─────────────┐
        │                            │
 Index Available?               No Index
        │                            │
        ▼                            ▼
 Index Scan                  Collection Scan
        │                            │
        └──────────────┬─────────────┘
                       ▼
                Result Processor
                       ▼
                  JSON Response
```

---

### Feature 1: Secondary Indexes [✅ DONE]
**Priority:** Critical
**Support:**
```sql
CREATE INDEX idx_name ON doctors(name);
CREATE INDEX idx_city ON doctors(city);
CREATE INDEX idx_speciality ON doctors(speciality);
```
**Internal Structure:** HashMap (Field -> Field Value -> Document IDs)
**Example:** `speciality -> Cardiology -> [100, 501, 720, 991]`
**Supported Types:** String, Integer, Float, Boolean, Date, UUID
**Requirements:** Automatic Updates (Insert -> Index Update, Update -> Old Value Remove / New Value Add, Delete -> Remove from Index)

### Feature 2: Composite Indexes [✅ DONE]
**Support:**
```sql
CREATE INDEX idx_city_speciality ON doctors(city,speciality);
```
**Internal:** `Hyderabad|Cardiology -> IDs`
**Useful for:** `WHERE city='Hyderabad' AND speciality='Cardiology'`

### Feature 3: Query Planner [✅ DONE]
**Current:** Execute WHERE in written order
**New:** Analyze Query -> Check Existing Indexes -> Estimate Cost -> Choose Fastest Plan -> Execute
**Planner Responsibilities:** Detect usable indexes, Estimate row count, Estimate execution cost, Pick best index, Avoid unnecessary scans

### Feature 4: Explain Command [✅ DONE]
**Support:**
```sql
EXPLAIN SELECT * FROM doctors WHERE city='Hyderabad';
```
**Output:** Execution Plan (Collection, Index Used, Estimated Rows, Estimated Cost, Execution Type)

### Feature 5: Background Index Builder [✅ DONE]
**Current:** Create Index -> Database Locked
**New:** Create Index -> Background Thread -> Continue Reads -> Continue Writes -> Complete

### Feature 6: Index Manager [✅ DONE]
**Commands:** `SHOW INDEXES;`, `DROP INDEX idx_city;`, `CREATE INDEX ...`

### Feature 7: Index Statistics [✅ DONE]
**Command:** `SHOW INDEX STATS;`

### Feature 8: Benchmark Suite [✅ DONE]
**Command:** `benchmark run`
Measure Insert, Update, Delete, Read, Indexed Read, Range Query for 100K to 100M docs.

### Feature 9: Query Optimizations
Automatically optimize WHERE, ORDER BY, LIMIT, COUNT, DISTINCT (Skip unnecessary fields, Early termination, Limit pushdown, Predicate pushdown)

### Feature 10: Memory Cache [✅ DONE]
**Maintain:** Hot Index Cache. Recently used indexes stay in memory for faster lookups.

### Feature 11: Index Recovery
**On startup:** Read Metadata -> Verify Index -> Repair if Needed -> Continue
**Crash recovery:** Index Corrupted -> Automatic Rebuild

### Feature 12: Metadata Storage [✅ DONE]
**Maintain:** Index Name, Collection, Fields, Unique, Created Date, Updated Date, Version

### Feature 13: API & SDK Support [✅ DONE]
**REST endpoints:** `POST /index/create`, `DELETE /index`, `GET /index/list`, `GET /index/stats`

### Feature 14: SDK Support [✅ DONE]
**Support:** Rust, Go, Node.js, Python, Flutter examples

### Feature 15: Testing [✅ DONE]
Ensure >95% code coverage for the indexing subsystem and Query Planner logic.

---

### Performance Goals
| Dataset | Current | Target |
|---------|---------|--------|
| 100K Docs | 150 ms | <10 ms |
| 1 Million | 1200 ms | <25 ms |
| 10 Million | 12 sec | <120 ms |
| 100 Million | 120 sec | <500 ms |

---

### File Structure
```text
pingdart-core/
src/
 index/
    manager.rs
    builder.rs
    btree.rs
    hashmap.rs
    metadata.rs
    cache.rs
    recovery.rs
 planner/
    planner.rs
    optimizer.rs
    explain.rs
 benchmark/
    benchmark.rs
 metadata/
    indexes.json
 tests/
    index_tests.rs
    planner_tests.rs
```

---

### Risks
- Increased memory usage due to indexes.
- Background indexing must remain consistent during concurrent writes.
- Query planner decisions need accurate statistics to avoid slower plans.

### Out of Scope (v0.2.0)
Distributed clustering, Replication, Sharding, ACID transactions, WAL, Full-text search, Vector search, Graph queries.
