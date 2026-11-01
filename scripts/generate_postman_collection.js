const fs = require('fs');
const path = require('path');

function createRequest({ name, method, pathSegments, queryParams = [], body = null, headers = [], auth = null, description = '', testScript = null }) {
    const cleanSegments = pathSegments.filter(p => p !== '');
    const urlObj = {
        raw: `{{baseUrl}}${cleanSegments.length > 0 ? '/' + cleanSegments.join('/') : ''}${queryParams.length > 0 ? '?' + queryParams.map(q => `${q.key}=${q.value}`).join('&') : ''}`,
        host: ['{{baseUrl}}'],
        path: cleanSegments,
    };

    if (queryParams.length > 0) {
        urlObj.query = queryParams.map(q => ({
            key: q.key,
            value: q.value,
            description: q.description || ''
        }));
    }

    const variableList = [];
    pathSegments.forEach(p => {
        if (p.startsWith(':')) {
            variableList.push({
                key: p.substring(1),
                value: p === ':table' ? 'users' : (p === ':id' ? '1' : (p === ':job_id' ? 'job_123' : (p === ':trace_id' ? 'trace_123' : 'item_1'))),
                description: `URL parameter for ${p}`
            });
        }
    });

    if (variableList.length > 0) {
        urlObj.variable = variableList;
    }

    const headerList = headers.map(h => ({
        key: h.key,
        value: h.value,
        type: 'text',
        description: h.description || ''
    }));

    let bodyObj = null;
    if (body) {
        if (body.mode === 'formdata') {
            bodyObj = {
                mode: 'formdata',
                formdata: body.formdata
            };
        } else {
            headerList.unshift({
                key: 'Content-Type',
                value: 'application/json',
                type: 'text'
            });
            bodyObj = {
                mode: 'raw',
                raw: typeof body === 'string' ? body : JSON.stringify(body, null, 2),
                options: {
                    raw: {
                        language: 'json'
                    }
                }
            };
        }
    }

    const events = [];
    if (testScript) {
        events.push({
            listen: 'test',
            script: {
                type: 'text/javascript',
                exec: testScript.split('\n')
            }
        });
    }

    const reqItem = {
        name,
        request: {
            method,
            header: headerList,
            url: urlObj,
            description: description || name
        },
        response: []
    };

    if (bodyObj) {
        reqItem.request.body = bodyObj;
    }
    if (auth) {
        reqItem.request.auth = auth;
    }
    if (events.length > 0) {
        reqItem.event = events;
    }

    return reqItem;
}

const collection = {
    info: {
        _postman_id: "c8942bfa-9481-4bb2-8924-f582299a9bce",
        name: "PingDartDB v0.5.0 Enterprise API Collection",
        description: "Official comprehensive Postman Collection for PingDartDB AI-Native Distributed Database Engine. Covers Universal SQL, DDL Tables/Columns, User Permissions, Document API, REST CRUD, HNSW Vectors, BM25 Hybrid Search, AI Cognitive Memory, Labeled Graph, Multi-Tenant Governance, Enterprise Security & RBAC, Universal Migration, Backup/Restore & PITR, Observability, and Storage Engine.",
        schema: "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
    },
    variable: [
        {
            key: "baseUrl",
            value: "http://localhost:4650",
            type: "string",
            description: "PingDartDB Server HTTP endpoint"
        },
        {
            key: "wsUrl",
            value: "ws://localhost:4650",
            type: "string",
            description: "PingDartDB Realtime WebSocket endpoint"
        },
        {
            key: "username",
            value: "ravi80181",
            type: "string",
            description: "Database root administrator username"
        },
        {
            key: "password",
            value: "RavindraChinnu12#",
            type: "string",
            description: "Database root administrator password"
        },
        {
            key: "authToken",
            value: "",
            type: "string",
            description: "Active session token auto-captured by /login"
        },
        {
            key: "database",
            value: "default",
            type: "string",
            description: "Active database scope"
        },
        {
            key: "tenantId",
            value: "default",
            type: "string",
            description: "Active multi-tenant context ID"
        }
    ],
    item: [
        // 1. Authentication
        {
            name: "1. Authentication & Session",
            description: "Login and token generation for root administrator and provisioned users.",
            item: [
                createRequest({
                    name: "1.1 Login as Root Admin (Auto-saves authToken)",
                    method: "POST",
                    pathSegments: ["login"],
                    body: {
                        username: "{{username}}",
                        password: "{{password}}"
                    },
                    description: "Authenticates with root admin credentials. The Postman test script automatically stores the returned session token into the collection variable `authToken` for subsequent requests.",
                    testScript: `var jsonData = pm.response.json();\npm.test("Status is success", function () {\n    pm.expect(jsonData.status).to.eql("success");\n});\nif (jsonData.token) {\n    pm.collectionVariables.set("authToken", jsonData.token);\n    console.log("Auth token updated in collection variables:", jsonData.token);\n}`
                })
            ]
        },
        // 2. User Management & Database Permissions
        {
            name: "2. User Management & Database Permissions",
            description: "Manage database user accounts, passwords, allowed IP addresses/hosts, and grant/revoke database permissions.",
            item: [
                createRequest({
                    name: "2.1 Show All Database Users",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "SHOW USERS;",
                        token: "{{authToken}}"
                    },
                    description: "Lists all database users, host masks, active roles, and assigned permissions."
                }),
                createRequest({
                    name: "2.2 Create Database User with Password & Host",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "CREATE USER 'app_developer'@'%' PASSWORD 'DeveloperPass2026!';",
                        token: "{{authToken}}"
                    },
                    description: "Creates a new database user account with password and host permission (e.g. '%' for any host or IP subnet)."
                }),
                createRequest({
                    name: "2.3 Grant Full Database Admin Permissions",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "GRANT ALL ON default TO app_developer;",
                        token: "{{authToken}}"
                    },
                    description: "Grants full administrative permissions on target database to the user."
                }),
                createRequest({
                    name: "2.4 Grant Read-Only (SELECT) Permissions",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "GRANT SELECT ON default TO app_developer;",
                        token: "{{authToken}}"
                    },
                    description: "Grants read-only access (SELECT queries) on target database to the user."
                }),
                createRequest({
                    name: "2.5 Grant Read-Write Permissions",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "GRANT READWRITE ON default TO app_developer;",
                        token: "{{authToken}}"
                    },
                    description: "Grants read and write access (SELECT, INSERT, UPDATE, DELETE) on target database."
                }),
                createRequest({
                    name: "2.6 Revoke Database Permissions",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "REVOKE ALL ON default FROM app_developer;",
                        token: "{{authToken}}"
                    },
                    description: "Revokes permissions on target database from the user."
                }),
                createRequest({
                    name: "2.7 Alter User Password & Allowed Host/IP",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "ALTER USER app_developer PASSWORD 'UpdatedSecretPass2026!' IP '%';",
                        token: "{{authToken}}"
                    },
                    description: "Changes user's password or updates allowed IP connection mask."
                }),
                createRequest({
                    name: "2.8 Drop Database User",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "DROP USER app_developer;",
                        token: "{{authToken}}"
                    },
                    description: "Deletes a database user account."
                })
            ]
        },
        // 3. Unified SQL Query Engine & DDL
        {
            name: "3. SQL Query Engine & DDL (Any Query API)",
            description: "Direct SQL execution engine. Pass ANY valid SQL statement (CREATE DATABASE, CREATE TABLE with columns, SHOW TABLES, INSERT, SELECT, JOINS, ALTER, DROP) directly in the 'query' field.",
            item: [
                createRequest({
                    name: "3.1 Run ANY Custom SQL Query (Universal Template)",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "SELECT 'PingDartDB Universal SQL API' as engine, NOW() as current_time;",
                        token: "{{authToken}}",
                        database: "{{database}}"
                    },
                    description: "Universal SQL Query endpoint: write and run ANY SQL command directly in the 'query' string parameter."
                }),
                createRequest({
                    name: "3.2 Show Databases",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "SHOW DATABASES;",
                        token: "{{authToken}}",
                        database: "{{database}}"
                    },
                    description: "Lists all registered databases in the engine."
                }),
                createRequest({
                    name: "3.3 Create Database",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "CREATE DATABASE enterprise_crm;",
                        token: "{{authToken}}"
                    },
                    description: "Creates a new logical database catalog."
                }),
                createRequest({
                    name: "3.4 Show Tables in Database",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "SHOW TABLES;",
                        token: "{{authToken}}",
                        database: "{{database}}"
                    },
                    description: "Lists all tables in the active database along with row count and storage engine type."
                }),
                createRequest({
                    name: "3.5 Create Table with Columns, Constraints & Types",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "CREATE TABLE customers (id INT AUTO_INCREMENT PRIMARY KEY, full_name STRING NOT NULL, email STRING UNIQUE, credit_score INT DEFAULT 700, balance FLOAT DEFAULT 0.0, is_verified BOOLEAN DEFAULT true, created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP);",
                        token: "{{authToken}}",
                        database: "{{database}}"
                    },
                    description: "Creates a structured relational table with custom column data types (INT, STRING, FLOAT, BOOLEAN, TIMESTAMP), AUTO_INCREMENT, PRIMARY KEY, NOT NULL, and DEFAULT constraints."
                }),
                createRequest({
                    name: "3.6 Insert Rows (DML)",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "INSERT INTO customers (full_name, email, credit_score, balance, is_verified) VALUES ('Sarah Connor', 'sarah@resistance.org', 820, 15400.50, true), ('John Connor', 'john@resistance.org', 750, 4200.00, true);",
                        token: "{{authToken}}",
                        database: "{{database}}"
                    },
                    description: "Inserts records into the specified table."
                }),
                createRequest({
                    name: "3.7 Select All Columns & Rows",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "SELECT * FROM customers;",
                        token: "{{authToken}}",
                        database: "{{database}}"
                    },
                    description: "Selects all columns and rows from table."
                }),
                createRequest({
                    name: "3.8 Select with WHERE, ORDER BY, LIMIT",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "SELECT id, full_name, email, balance FROM customers WHERE balance > 5000.0 ORDER BY balance DESC LIMIT 10;",
                        token: "{{authToken}}",
                        database: "{{database}}"
                    },
                    description: "Executes filtered, sorted, and paginated SELECT query."
                }),
                createRequest({
                    name: "3.9 Aggregation & GROUP BY",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "SELECT is_verified, COUNT(*) as total_users, AVG(balance) as avg_balance, SUM(balance) as total_balance FROM customers GROUP BY is_verified;",
                        token: "{{authToken}}",
                        database: "{{database}}"
                    },
                    description: "Analytical aggregation query using vectorized columnar engine."
                }),
                createRequest({
                    name: "3.10 Explain Query Plan (CBO)",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "EXPLAIN ANALYZE SELECT * FROM customers WHERE email = 'sarah@resistance.org';",
                        token: "{{authToken}}",
                        database: "{{database}}"
                    },
                    description: "Inspects Cost-Based Optimizer (CBO) plan selection, index usage, and execution timing."
                }),
                createRequest({
                    name: "3.11 Drop Table",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "DROP TABLE customers;",
                        token: "{{authToken}}",
                        database: "{{database}}"
                    },
                    description: "Drops table and cleans up data, schema, and secondary indexes."
                }),
                createRequest({
                    name: "3.12 Drop Database",
                    method: "POST",
                    pathSegments: ["query"],
                    body: {
                        query: "DROP DATABASE enterprise_crm;",
                        token: "{{authToken}}"
                    },
                    description: "Drops an entire database catalog and all its associated tables."
                }),
                createRequest({
                    name: "3.13 PRD REST Query (/v1/query)",
                    method: "POST",
                    pathSegments: ["v1", "query"],
                    headers: [
                        { key: "Authorization", value: "Bearer {{authToken}}" }
                    ],
                    body: {
                        query: "SELECT * FROM customers LIMIT 10",
                        database: "{{database}}"
                    },
                    description: "Executes structured query via PRD Section 6.5 REST interface."
                }),
                createRequest({
                    name: "3.14 PRD Data Query (/v1/data/query)",
                    method: "POST",
                    pathSegments: ["v1", "data", "query"],
                    headers: [
                        { key: "Authorization", value: "Bearer {{authToken}}" }
                    ],
                    body: {
                        query: "SELECT * FROM customers WHERE credit_score >= 700",
                        database: "{{database}}"
                    },
                    description: "Executes structured query via alternate data endpoint."
                })
            ]
        },
        // 4. Document Store API
        {
            name: "4. Document Store API (Firebase / Mongo)",
            description: "NoSQL document filtering, geospatial proximity queries, and flexible schema lookups.",
            item: [
                createRequest({
                    name: "4.1 Document Filter Query",
                    method: "POST",
                    pathSegments: ["document", "query"],
                    body: {
                        token: "{{authToken}}",
                        database: "{{database}}",
                        collection: "customers",
                        where: [
                            { field: "is_verified", operator: "==", value: "true" }
                        ],
                        order_by: "id ASC",
                        limit: 10,
                        offset: 0,
                        explain: false
                    },
                    description: "Queries records in a collection using declarative filters."
                }),
                createRequest({
                    name: "4.2 Geospatial Proximity Query (near)",
                    method: "POST",
                    pathSegments: ["document", "query"],
                    body: {
                        token: "{{authToken}}",
                        database: "{{database}}",
                        collection: "branches",
                        near: {
                            lat: 37.7749,
                            lon: -122.4194,
                            radius_km: 25.0
                        },
                        limit: 5
                    },
                    description: "Finds entities located within radius_km using spatial geohash indexing."
                })
            ]
        },
        // 5. REST Data CRUD
        {
            name: "5. REST Data CRUD (Directus / Supabase)",
            description: "Direct REST table endpoints supporting postgREST-style filtering operators (eq, gt, in, like, etc.).",
            item: [
                createRequest({
                    name: "5.1 List Records (with filters)",
                    method: "GET",
                    pathSegments: ["v1", "data", ":table"],
                    queryParams: [
                        { key: "database", value: "{{database}}" },
                        { key: "limit", value: "20" },
                        { key: "offset", value: "0" },
                        { key: "order", value: "id.desc" },
                        { key: "select", value: "id,full_name,email,balance" }
                    ],
                    headers: [
                        { key: "Authorization", value: "Bearer {{authToken}}" }
                    ],
                    description: "Lists rows from table with paging and field projection."
                }),
                createRequest({
                    name: "5.2 Insert Record",
                    method: "POST",
                    pathSegments: ["v1", "data", ":table"],
                    queryParams: [
                        { key: "database", value: "{{database}}" }
                    ],
                    headers: [
                        { key: "Authorization", value: "Bearer {{authToken}}" }
                    ],
                    body: {
                        id: 10,
                        full_name: "Dana Scully",
                        email: "dana@fbi.gov",
                        balance: 110000.0,
                        is_verified: true
                    },
                    description: "Inserts a new row or document into the table."
                }),
                createRequest({
                    name: "5.3 Get Record by ID",
                    method: "GET",
                    pathSegments: ["v1", "data", ":table", ":id"],
                    queryParams: [
                        { key: "database", value: "{{database}}" }
                    ],
                    headers: [
                        { key: "Authorization", value: "Bearer {{authToken}}" }
                    ],
                    description: "Fetches a single row by primary key ID."
                }),
                createRequest({
                    name: "5.4 Update Record (PATCH)",
                    method: "PATCH",
                    pathSegments: ["v1", "data", ":table", ":id"],
                    queryParams: [
                        { key: "database", value: "{{database}}" }
                    ],
                    headers: [
                        { key: "Authorization", value: "Bearer {{authToken}}" }
                    ],
                    body: {
                        balance: 135000.0
                    },
                    description: "Updates specified columns of a row."
                }),
                createRequest({
                    name: "5.5 Delete Record",
                    method: "DELETE",
                    pathSegments: ["v1", "data", ":table", ":id"],
                    queryParams: [
                        { key: "database", value: "{{database}}" }
                    ],
                    headers: [
                        { key: "Authorization", value: "Bearer {{authToken}}" }
                    ],
                    description: "Soft/hard deletes a row by ID."
                })
            ]
        },
        // 6. Schema & Table Metadata
        {
            name: "6. Schema & Table Metadata",
            description: "Table schema definitions, column listings, and catalog management.",
            item: [
                createRequest({
                    name: "6.1 List All Tables",
                    method: "GET",
                    pathSegments: ["v1", "schema", "tables"],
                    queryParams: [
                        { key: "database", value: "{{database}}" }
                    ],
                    headers: [
                        { key: "Authorization", value: "Bearer {{authToken}}" }
                    ],
                    description: "Returns metadata for all tables in the database."
                }),
                createRequest({
                    name: "6.2 Create Table via Schema API",
                    method: "POST",
                    pathSegments: ["v1", "schema", "tables"],
                    queryParams: [
                        { key: "database", value: "{{database}}" }
                    ],
                    headers: [
                        { key: "Authorization", value: "Bearer {{authToken}}" }
                    ],
                    body: {
                        table_name: "products",
                        columns: [
                            { name: "id", data_type: "INT", primary_key: true },
                            { name: "sku", data_type: "STRING", nullable: false },
                            { name: "title", data_type: "STRING", nullable: false },
                            { name: "price", data_type: "FLOAT", nullable: false },
                            { name: "in_stock", data_type: "BOOLEAN", default_value: "true" }
                        ]
                    },
                    description: "Creates a new schema definition programmatically."
                }),
                createRequest({
                    name: "6.3 Get Table Schema",
                    method: "GET",
                    pathSegments: ["v1", "schema", "tables", ":table"],
                    queryParams: [
                        { key: "database", value: "{{database}}" }
                    ],
                    headers: [
                        { key: "Authorization", value: "Bearer {{authToken}}" }
                    ],
                    description: "Retrieves column and constraint details for a specific table."
                }),
                createRequest({
                    name: "6.4 Truncate Table",
                    method: "POST",
                    pathSegments: ["v1", "schema", "tables", ":table", "truncate"],
                    queryParams: [
                        { key: "database", value: "{{database}}" }
                    ],
                    headers: [
                        { key: "Authorization", value: "Bearer {{authToken}}" }
                    ],
                    description: "Clears all records in a table while retaining schema definition."
                }),
                createRequest({
                    name: "6.5 Drop Table",
                    method: "DELETE",
                    pathSegments: ["v1", "schema", "tables", ":table"],
                    queryParams: [
                        { key: "database", value: "{{database}}" }
                    ],
                    headers: [
                        { key: "Authorization", value: "Bearer {{authToken}}" }
                    ],
                    description: "Drops table and associated index files."
                })
            ]
        },
        // 7. Vector Engine & Search
        {
            name: "7. Vector Engine & AI Search",
            description: "HNSW approximate nearest neighbor search, BM25 full-text indexing, and reciprocal rank fusion hybrid search.",
            item: [
                createRequest({
                    name: "7.1 HNSW Vector Search",
                    method: "POST",
                    pathSegments: ["vector", "search"],
                    body: {
                        token: "{{authToken}}",
                        database: "{{database}}",
                        table: "documents",
                        column: "embedding",
                        vector: [0.034, -0.125, 0.442, -0.012, 0.771],
                        top_k: 5
                    },
                    description: "Executes vector cosine/Euclidean distance search over HNSW index."
                }),
                createRequest({
                    name: "7.2 BM25 Full-Text Search",
                    method: "POST",
                    pathSegments: ["search", "text"],
                    body: {
                        token: "{{authToken}}",
                        database: "{{database}}",
                        table: "documents",
                        column: "content",
                        query: "high-throughput transactional storage",
                        top_k: 5
                    },
                    description: "Performs BM25 ranked full-text search."
                }),
                createRequest({
                    name: "7.3 Hybrid Search (Vector + BM25)",
                    method: "POST",
                    pathSegments: ["search", "hybrid"],
                    body: {
                        token: "{{authToken}}",
                        database: "{{database}}",
                        table: "documents",
                        vector_column: "embedding",
                        text_column: "content",
                        vector: [0.034, -0.125, 0.442, -0.012, 0.771],
                        query: "transactional storage engine",
                        alpha: 0.65,
                        top_k: 5
                    },
                    description: "Combines dense vector and sparse lexical results via alpha weighting (0.0 = BM25 only, 1.0 = Vector only)."
                })
            ]
        },
        // 8. AI Cognitive Memory & Agents
        {
            name: "8. AI Cognitive Memory & Agents",
            description: "Episodic, Semantic, Working, and Long-Term cognitive memory subsystem with automatic sleep consolidation.",
            item: [
                createRequest({
                    name: "8.1 Store Agent Memory",
                    method: "POST",
                    pathSegments: ["ai", "memory", "store"],
                    body: {
                        token: "{{authToken}}",
                        session_id: "agent_session_77",
                        memory_type: "Episodic",
                        content: "User requested automated daily backups at 02:00 UTC with AES-256-GCM encryption.",
                        importance: 0.95,
                        metadata: {
                            category: "user_preference",
                            component: "backup_system"
                        }
                    },
                    description: "Stores an experience or fact into the AI cognitive memory store."
                }),
                createRequest({
                    name: "8.2 Recall Agent Memory",
                    method: "POST",
                    pathSegments: ["ai", "memory", "recall"],
                    body: {
                        token: "{{authToken}}",
                        session_id: "agent_session_77",
                        memory_type: "Episodic",
                        top_k: 5
                    },
                    description: "Retrieves relevant memories based on contextual session similarity."
                }),
                createRequest({
                    name: "8.3 Consolidate Working Memory",
                    method: "POST",
                    pathSegments: ["ai", "memory", "consolidate"],
                    body: {
                        token: "{{authToken}}",
                        session_id: "agent_session_77"
                    },
                    description: "Triggers memory consolidation to compress ephemeral observations into long-term semantic knowledge."
                })
            ]
        },
        // 9. Graph Database Engine
        {
            name: "9. Graph Database Engine (Neo4j Style)",
            description: "Labeled Property Graph engine supporting nodes, directed typed edges, neighbor traversal, and shortest path finding.",
            item: [
                createRequest({
                    name: "9.1 Create/Update Graph Node",
                    method: "POST",
                    pathSegments: ["graph", "node"],
                    body: {
                        token: "{{authToken}}",
                        id: "srv_cluster_leader",
                        labels: ["Server", "Coordinator"],
                        properties: {
                            hostname: "node-1.pingdart.internal",
                            region: "us-east-1",
                            ram_gb: 128
                        }
                    },
                    description: "Upserts a graph node with labels and properties."
                }),
                createRequest({
                    name: "9.2 Create Directed Graph Edge",
                    method: "POST",
                    pathSegments: ["graph", "edge"],
                    body: {
                        token: "{{authToken}}",
                        id: "edge_replicates_to",
                        source: "srv_cluster_leader",
                        target: "srv_cluster_replica",
                        type: "REPLICATES_TO",
                        weight: 1.0,
                        properties: {
                            sync_mode: "streaming",
                            max_lag_ms: 5
                        }
                    },
                    description: "Creates a directed relationship between two nodes."
                }),
                createRequest({
                    name: "9.3 Find Shortest Path",
                    method: "POST",
                    pathSegments: ["graph", "path"],
                    body: {
                        token: "{{authToken}}",
                        start: "srv_cluster_leader",
                        end: "srv_cluster_replica",
                        max_hops: 5
                    },
                    description: "Calculates the shortest graph traversal path between two nodes."
                }),
                createRequest({
                    name: "9.4 Query Node Neighbors",
                    method: "POST",
                    pathSegments: ["graph", "neighbors"],
                    body: {
                        token: "{{authToken}}",
                        node_id: "srv_cluster_leader",
                        edge_type: "REPLICATES_TO",
                        target_label: "Server"
                    },
                    description: "Returns adjacent neighbors of a node filtered by edge type and target label."
                })
            ]
        },
        // 10. Secondary Indexes
        {
            name: "10. Indexes & Secondary Indexing",
            description: "B+Tree secondary and unique indexing for sub-millisecond point lookups and range scans.",
            item: [
                createRequest({
                    name: "10.1 Create Secondary Index",
                    method: "POST",
                    pathSegments: ["index", "create"],
                    body: {
                        token: "{{authToken}}",
                        database: "{{database}}",
                        table: "customers",
                        fields: ["email", "credit_score"]
                    },
                    description: "Builds a composite B+Tree secondary index on specified table fields."
                }),
                createRequest({
                    name: "10.2 List Table Indexes",
                    method: "POST",
                    pathSegments: ["index", "list"],
                    body: {
                        token: "{{authToken}}",
                        database: "{{database}}"
                    },
                    description: "Lists all active B+Tree indexes in the target database."
                }),
                createRequest({
                    name: "10.3 Index Operational Stats",
                    method: "POST",
                    pathSegments: ["index", "stats"],
                    body: {
                        token: "{{authToken}}",
                        database: "{{database}}"
                    },
                    description: "Returns page split, depth, and entry counts for indexes."
                }),
                createRequest({
                    name: "10.4 Drop Index",
                    method: "POST",
                    pathSegments: ["index", "drop"],
                    body: {
                        token: "{{authToken}}",
                        database: "{{database}}",
                        table: "customers",
                        fields: ["email", "credit_score"]
                    },
                    description: "Deallocates index structure and frees disk space."
                })
            ]
        },
        // 11. CDC Replay
        {
            name: "11. CDC & Change Data Capture",
            description: "Durable WAL event streaming and downstream consumer replay.",
            item: [
                createRequest({
                    name: "11.1 Replay CDC Log Events",
                    method: "POST",
                    pathSegments: ["cdc", "replay"],
                    body: {
                        token: "{{authToken}}",
                        since_sequence_id: 0,
                        limit: 50
                    },
                    description: "Replays mutation events from the durable write-ahead commit log."
                })
            ]
        },
        // 12. Multi-Tenant Governance
        {
            name: "12. Multi-Tenant Governance & RLS",
            description: "Hardware tenant quota isolation (Storage, Vectors, QPS) and Row-Level Security policies.",
            item: [
                createRequest({
                    name: "12.1 Create Tenant with Quotas",
                    method: "POST",
                    pathSegments: ["tenant", "create"],
                    body: {
                        token: "{{authToken}}",
                        tenant_id: "tenant_fintech_corp",
                        name: "FinTech Enterprise Corp",
                        max_storage_mb: 20480,
                        max_vectors: 2000000,
                        max_qps: 500
                    },
                    description: "Provisions an isolated tenant container with hard enforcement limits."
                }),
                createRequest({
                    name: "12.2 List All Tenants",
                    method: "POST",
                    pathSegments: ["tenant", "list"],
                    body: {
                        token: "{{authToken}}"
                    },
                    description: "Returns all registered tenants and their quota utilization."
                }),
                createRequest({
                    name: "12.3 Register RLS Policy",
                    method: "POST",
                    pathSegments: ["tenant", "policy"],
                    body: {
                        token: "{{authToken}}",
                        name: "rls_fintech_isolation",
                        table: "transactions",
                        command: "ALL",
                        using_expr: "tenant_id = current_tenant()"
                    },
                    description: "Configures row-level security policy restricting data visibility by active tenant."
                }),
                createRequest({
                    name: "12.4 Toggle RLS on Table",
                    method: "POST",
                    pathSegments: ["tenant", "rls"],
                    body: {
                        token: "{{authToken}}",
                        table: "transactions",
                        enable: true
                    },
                    description: "Enables or disables Row-Level Security evaluation on a table."
                })
            ]
        },
        // 13. Cluster & Raft Consensus
        {
            name: "13. Cluster & Raft Consensus",
            description: "Raft consensus engine, cluster membership, consistent hash sharding, and multi-node topology.",
            item: [
                createRequest({
                    name: "13.1 Cluster Status & Topology",
                    method: "GET",
                    pathSegments: ["cluster", "status"],
                    description: "Returns cluster member list, Raft state (Leader/Follower), term, and shard allocations."
                }),
                createRequest({
                    name: "13.2 Join Node to Cluster Seed",
                    method: "POST",
                    pathSegments: ["cluster", "join"],
                    body: {
                        seed_address: "127.0.0.1:4651"
                    },
                    description: "Directs current node to join an existing Raft cluster via seed address."
                }),
                createRequest({
                    name: "13.3 Shard Scatter-Gather Vector Search",
                    method: "POST",
                    pathSegments: ["cluster", "shard", "vector_search"],
                    body: {
                        table: "documents",
                        column: "embedding",
                        vector: [0.034, -0.125, 0.442, -0.012, 0.771],
                        top_k: 5
                    },
                    description: "Executes partition-level vector search across cluster shards."
                })
            ]
        },
        // 14. Enterprise Security & Access Control
        {
            name: "14. Enterprise Security & Access Control",
            description: "Granular RBAC roles, scoped API key lifecycle, and AES-256-GCM envelope cryptographic service.",
            item: [
                createRequest({
                    name: "14.1 Create Scoped API Key",
                    method: "POST",
                    pathSegments: ["security", "apikey", "create"],
                    body: {
                        token: "{{authToken}}",
                        name: "bi_dashboard_service_key",
                        roles: ["TableRead", "VectorSearch"],
                        tenant_id: "default",
                        ttl_secs: 2592000
                    },
                    description: "Issues a revocable service API key (prefix pd_live_)."
                }),
                createRequest({
                    name: "14.2 List Active API Keys",
                    method: "POST",
                    pathSegments: ["security", "apikey", "list"],
                    body: {
                        token: "{{authToken}}"
                    },
                    description: "Lists all provisioned API key metadata (secrets remain hashed)."
                }),
                createRequest({
                    name: "14.3 Revoke API Key",
                    method: "POST",
                    pathSegments: ["security", "apikey", "revoke"],
                    body: {
                        token: "{{authToken}}",
                        key_id: "key_replace_with_id"
                    },
                    description: "Immediately revokes an active API key."
                }),
                createRequest({
                    name: "14.4 Create Custom RBAC Role",
                    method: "POST",
                    pathSegments: ["security", "role", "create"],
                    body: {
                        token: "{{authToken}}",
                        name: "DataAnalystRole",
                        description: "Read-only access to analytics tables and vectors",
                        permissions: ["TableRead", "VectorSearch", "AuditRead"],
                        scope: "global"
                    },
                    description: "Defines a custom role with fine-grained permission flags."
                }),
                createRequest({
                    name: "14.5 List RBAC Roles",
                    method: "GET",
                    pathSegments: ["security", "role", "list"],
                    description: "Lists all system and custom RBAC roles."
                }),
                createRequest({
                    name: "14.6 Envelope Encrypt Payload (AES-256-GCM)",
                    method: "POST",
                    pathSegments: ["security", "encrypt"],
                    body: {
                        token: "{{authToken}}",
                        plaintext: "Sensitive healthcare patient records or payment token"
                    },
                    description: "Encrypts arbitrary plaintext using engine's master envelope key."
                }),
                createRequest({
                    name: "14.7 Envelope Decrypt Payload (AES-256-GCM)",
                    method: "POST",
                    pathSegments: ["security", "decrypt"],
                    body: {
                        token: "{{authToken}}",
                        ciphertext_hex: "paste_hex_output_from_encrypt_here"
                    },
                    description: "Decrypts ciphertext hex string back into original plaintext."
                })
            ]
        },
        // 15. Universal Migration Framework
        {
            name: "15. Universal Migration Framework",
            description: "Zero-downtime ETL migration pipeline supporting MySQL dumps, PostgreSQL, MongoDB extended JSON, and CSV.",
            item: [
                createRequest({
                    name: "15.1 Start Migration Job",
                    method: "POST",
                    pathSegments: ["migration", "start"],
                    body: {
                        token: "{{authToken}}",
                        source_type: "mysql_dump",
                        source_path: "sql/meddylist_sample.sql",
                        target_table: "migrated_clinics",
                        schema_policy: "Lenient",
                        primary_key: "id",
                        batch_size: 500,
                        workers: 4
                    },
                    description: "Spawns an asynchronous background migration job."
                }),
                createRequest({
                    name: "15.2 List All Migration Jobs",
                    method: "GET",
                    pathSegments: ["migration", "jobs"],
                    description: "Returns all historical and running migration jobs."
                }),
                createRequest({
                    name: "15.3 Get Migration Job Status",
                    method: "GET",
                    pathSegments: ["migration", "status", ":job_id"],
                    description: "Fetches live progress (processed rows, failed rows, throughput)."
                }),
                createRequest({
                    name: "15.4 Validate Migration Integrity",
                    method: "GET",
                    pathSegments: ["migration", "validate", ":job_id"],
                    description: "Verifies source-to-target checksums and schema conformity."
                }),
                createRequest({
                    name: "15.5 Resume Paused Migration",
                    method: "POST",
                    pathSegments: ["migration", "resume", ":job_id"],
                    body: {
                        token: "{{authToken}}"
                    },
                    description: "Resumes interrupted migration from last durable checkpoint."
                }),
                createRequest({
                    name: "15.6 Abort Migration Job",
                    method: "POST",
                    pathSegments: ["migration", "abort", ":job_id"],
                    body: {
                        token: "{{authToken}}"
                    },
                    description: "Cancels an ongoing migration job."
                })
            ]
        },
        // 16. Backup & Disaster Recovery
        {
            name: "16. Backup & Disaster Recovery",
            description: "Online point-in-time snapshots, PITR transaction replay, and encrypted Protobuf (.pb) packaging.",
            item: [
                createRequest({
                    name: "16.1 Create Online Snapshot",
                    method: "POST",
                    pathSegments: ["backup", "snapshot"],
                    body: {
                        token: "{{authToken}}"
                    },
                    description: "Freezes WAL and creates a verifiable storage snapshot."
                }),
                createRequest({
                    name: "16.2 Restore Snapshot",
                    method: "POST",
                    pathSegments: ["backup", "restore"],
                    body: {
                        token: "{{authToken}}",
                        snapshot_id: "snapshot_replace_with_id"
                    },
                    description: "Restores entire database state from a snapshot."
                }),
                createRequest({
                    name: "16.3 Point-In-Time Recovery (PITR)",
                    method: "POST",
                    pathSegments: ["backup", "pitr"],
                    body: {
                        token: "{{authToken}}",
                        target_timestamp: 1740000000000
                    },
                    description: "Replays WAL transactions up to an exact epoch millisecond timestamp."
                }),
                createRequest({
                    name: "16.4 Export Protobuf (.pb) Encrypted Archive",
                    method: "POST",
                    pathSegments: ["backup", "export"],
                    body: {
                        token: "{{authToken}}",
                        export_type: "database",
                        database: "{{database}}",
                        file_path: "backups/prod_backup_export.pb",
                        encrypt: true,
                        key: "PingDartMasterSecurityPassphrase2026!"
                    },
                    description: "Packages table data into a ZSTD-compressed, AES-256 encrypted .pb container."
                }),
                createRequest({
                    name: "16.5 Import Protobuf (.pb) Archive",
                    method: "POST",
                    pathSegments: ["backup", "import"],
                    body: {
                        token: "{{authToken}}",
                        file_path: "backups/prod_backup_export.pb",
                        key: "PingDartMasterSecurityPassphrase2026!",
                        target_db: "{{database}}"
                    },
                    description: "Validates checksum, decrypts, and unpacks .pb backup."
                }),
                createRequest({
                    name: "16.6 Database JSON Dump Export",
                    method: "POST",
                    pathSegments: ["database", "export"],
                    body: {
                        token: "{{authToken}}",
                        database: "{{database}}"
                    },
                    description: "Exports all tables as portable JSON documents."
                }),
                createRequest({
                    name: "16.7 Database JSON Dump Import",
                    method: "POST",
                    pathSegments: ["database", "import"],
                    body: {
                        token: "{{authToken}}",
                        database: "{{database}}",
                        data: {}
                    },
                    description: "Imports tables from JSON dump structure."
                })
            ]
        },
        // 17. Observability & Health
        {
            name: "17. Observability, Health & Diagnostics",
            description: "Prometheus metrics, OpenTelemetry distributed traces, and slow query heuristics.",
            item: [
                createRequest({
                    name: "17.1 Engine Status & Capabilities",
                    method: "GET",
                    pathSegments: [""],
                    description: "Returns high-level engine operational status and feature flags."
                }),
                createRequest({
                    name: "17.2 Kubernetes Liveness Probe",
                    method: "GET",
                    pathSegments: ["health", "live"],
                    description: "K8s liveness probe returning UP status."
                }),
                createRequest({
                    name: "17.3 Kubernetes Readiness Probe",
                    method: "GET",
                    pathSegments: ["health", "ready"],
                    description: "K8s readiness probe verifying Raft leader election and storage engine readiness."
                }),
                createRequest({
                    name: "17.4 Prometheus Metrics Scraping",
                    method: "GET",
                    pathSegments: ["metrics"],
                    description: "Standard Prometheus /metrics scrape endpoint."
                }),
                createRequest({
                    name: "17.5 List OpenTelemetry Trace Spans",
                    method: "GET",
                    pathSegments: ["observability", "traces"],
                    queryParams: [
                        { key: "limit", value: "50" }
                    ],
                    description: "Lists recent distributed traces."
                }),
                createRequest({
                    name: "17.6 Get Trace Span Details",
                    method: "GET",
                    pathSegments: ["observability", "traces", ":trace_id"],
                    description: "Inspects full call tree and execution latency of a trace span."
                }),
                createRequest({
                    name: "17.7 List Slow Queries & Heuristics",
                    method: "GET",
                    pathSegments: ["observability", "slow_queries"],
                    queryParams: [
                        { key: "limit", value: "50" },
                        { key: "tenant", value: "default" }
                    ],
                    description: "Lists queries that exceeded slow query threshold with index optimization hints."
                })
            ]
        },
        // 18. Storage Engine (HBSE)
        {
            name: "18. Storage Engine (HBSE File Management)",
            description: "Hybrid Block Storage Engine for object, image, and document binary storage with deduplication and CDC tracking.",
            item: [
                createRequest({
                    name: "18.1 Upload Binary File (Multipart)",
                    method: "POST",
                    pathSegments: ["v1", "storage", "upload"],
                    body: {
                        mode: "formdata",
                        formdata: [
                            {
                                key: "file",
                                type: "file",
                                description: "Select any file or media to upload"
                            }
                        ]
                    },
                    description: "Uploads a binary file to the storage engine."
                }),
                createRequest({
                    name: "18.2 List Uploaded Files",
                    method: "GET",
                    pathSegments: ["v1", "storage", "files"],
                    description: "Returns metadata for all files stored in the engine."
                }),
                createRequest({
                    name: "18.3 Get File Metadata",
                    method: "GET",
                    pathSegments: ["v1", "storage", "files", ":id"],
                    description: "Retrieves size, MIME type, compression ratio, and download URL for a file."
                }),
                createRequest({
                    name: "18.4 Download File Content",
                    method: "GET",
                    pathSegments: ["v1", "storage", "files", ":id", "download"],
                    description: "Streams file binary directly."
                }),
                createRequest({
                    name: "18.5 Delete File",
                    method: "DELETE",
                    pathSegments: ["v1", "storage", "files", ":id"],
                    description: "Deletes file binary and metadata."
                })
            ]
        },
        // 19. Realtime WebSockets & Live Subscriptions
        {
            name: "19. Realtime WebSockets & Live Subscriptions (Firebase-like)",
            description: "Live CDC table change events, presence tracking, and ephemeral channel broadcasts over WebSockets (ws://localhost:4650/v1/realtime or /ws).",
            item: [
                createRequest({
                    name: "19.1 Connect to Realtime Gateway (WebSocket)",
                    method: "GET",
                    pathSegments: ["v1", "realtime"],
                    queryParams: [
                        { key: "token", value: "{{authToken}}" }
                    ],
                    headers: [
                        { key: "Upgrade", value: "websocket" },
                        { key: "Connection", value: "Upgrade" }
                    ],
                    description: "Connect to the live WebSocket gateway at ws://localhost:4650/v1/realtime?token={{authToken}} or ws://localhost:4650/ws"
                }),
                createRequest({
                    name: "19.2 Authenticate Session Frame (action: auth)",
                    method: "POST",
                    pathSegments: ["v1", "realtime"],
                    body: {
                        action: "auth",
                        token: "{{authToken}}"
                    },
                    description: "Sends an authentication frame over WebSocket if token was not provided in initial handshake URL query params."
                }),
                createRequest({
                    name: "19.3 Live Subscribe to Table (action: subscribe)",
                    method: "POST",
                    pathSegments: ["v1", "realtime"],
                    body: {
                        action: "subscribe",
                        table: "customers",
                        database: "{{database}}",
                        fetch_snapshot: true,
                        filter: {
                            is_verified: "true"
                        },
                        primary_key: "id"
                    },
                    description: "Subscribes to live CDC table changes (INSERT, UPDATE, DELETE). If fetch_snapshot is true, immediately receives initial table state snapshot."
                }),
                createRequest({
                    name: "19.4 Unsubscribe from Table (action: unsubscribe)",
                    method: "POST",
                    pathSegments: ["v1", "realtime"],
                    body: {
                        action: "unsubscribe",
                        table: "customers"
                    },
                    description: "Cancels active subscription to table events."
                }),
                createRequest({
                    name: "19.5 Broadcast Ephemeral Channel Message (action: broadcast)",
                    method: "POST",
                    pathSegments: ["v1", "realtime"],
                    body: {
                        action: "broadcast",
                        channel: "chat_room_alpha",
                        payload: {
                            user_id: "ravi80181",
                            event: "typing",
                            text: "Ravi is typing a message..."
                        }
                    },
                    description: "Broadcasts an ephemeral message to all clients in a channel without writing to persistent storage."
                }),
                createRequest({
                    name: "19.6 Track Online Presence State (action: presence_track)",
                    method: "POST",
                    pathSegments: ["v1", "realtime"],
                    body: {
                        action: "presence_track",
                        channel: "global_lobby",
                        user_id: "ravi80181",
                        state: {
                            status: "online",
                            device: "desktop",
                            client_version: "v0.5.0"
                        }
                    },
                    description: "Tracks online status. The server broadcasts presence_state and presence_diff (joins/leaves) to all channel participants."
                }),
                createRequest({
                    name: "19.7 Leave Presence Channel (action: presence_leave)",
                    method: "POST",
                    pathSegments: ["v1", "realtime"],
                    body: {
                        action: "presence_leave",
                        channel: "global_lobby"
                    },
                    description: "Leaves a presence channel. Notifies all participants of departure."
                }),
                createRequest({
                    name: "19.8 Heartbeat Ping Frame (action: ping)",
                    method: "POST",
                    pathSegments: ["v1", "realtime"],
                    body: {
                        action: "ping"
                    },
                    description: "Sends a keep-alive ping frame. The server responds immediately with a 'pong' event and epoch timestamp."
                })
            ]
        }
    ]
};

const outputPath = path.resolve(__dirname, '../PingDartDB.postman_collection.json');
fs.writeFileSync(outputPath, JSON.stringify(collection, null, 2), 'utf-8');
console.log(`Successfully generated Postman Collection at: ${outputPath}`);
console.log(`Total Folders: ${collection.item.length}`);
let totalReqs = 0;
collection.item.forEach(f => {
    totalReqs += f.item.length;
    console.log(` - ${f.name} (${f.item.length} requests)`);
});
console.log(`Total Requests: ${totalReqs}`);
