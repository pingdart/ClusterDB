# PingDart-Core SDK & REST Examples

PingDart-Core v0.2.0 exposes dedicated REST endpoints for Index Management. This allows your backend services to effortlessly manage database indexes to optimize queries on the fly.

Below are examples of how to interact with the new Indexing endpoints (`/index/create`, `/index/drop`, `/index/list`, `/index/stats`) using various languages.

## 1. Node.js (JavaScript/TypeScript)

```javascript
const axios = require('axios');

const PINGDART_URL = "http://localhost:8080";
const TOKEN = "your_admin_token";
const DB = "my_database";

// Create an Index
async function createIndex() {
    const response = await axios.post(`${PINGDART_URL}/index/create`, {
        token: TOKEN,
        database: DB,
        table: "users",
        fields: ["city", "age"] // Creates a composite index!
    });
    console.log("Index Created:", response.data);
}

// Get Index Statistics
async function getIndexStats() {
    const response = await axios.post(`${PINGDART_URL}/index/stats`, {
        token: TOKEN,
        database: DB
    });
    console.log("Index Hits:", response.data);
}
```

## 2. Go (Golang)

```go
package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
)

func main() {
	url := "http://localhost:8080/index/create"
	payload := map[string]interface{}{
		"token":    "your_admin_token",
		"database": "my_database",
		"table":    "users",
		"fields":   []string{"city", "age"},
	}

	jsonValue, _ := json.Marshal(payload)
	resp, err := http.Post(url, "application/json", bytes.NewBuffer(jsonValue))
	
	if err != nil {
		panic(err)
	}
	defer resp.Body.Close()

	fmt.Println("Index creation status:", resp.Status)
}
```

## 3. Python

```python
import requests

url = "http://localhost:8080/index/drop"
payload = {
    "token": "your_admin_token",
    "database": "my_database",
    "table": "users",
    "fields": ["city", "age"]
}

response = requests.post(url, json=payload)
print(response.json())
```

## 4. Rust

```rust
use reqwest::Client;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();
    let res = client.post("http://localhost:8080/index/list")
        .json(&json!({
            "token": "your_admin_token",
            "database": "my_database"
        }))
        .send()
        .await?;

    println!("Active Indexes: {:?}", res.text().await?);
    Ok(())
}
```
