use prost::Message;
use std::collections::HashMap;

#[derive(Clone, PartialEq, Message)]
pub struct PbPackageHeader {
    #[prost(string, tag = "1")]
    pub magic: String,
    #[prost(uint32, tag = "2")]
    pub version: u32,
    #[prost(uint64, tag = "3")]
    pub created_at: u64,
    #[prost(string, tag = "4")]
    pub scope: String, // "database" | "cluster"
    #[prost(string, tag = "5")]
    pub database: String,
    #[prost(bool, tag = "6")]
    pub is_encrypted: bool,
    #[prost(string, tag = "7")]
    pub compression: String, // "zstd"
    #[prost(string, tag = "8")]
    pub checksum_sha256: String,
    #[prost(uint64, tag = "9")]
    pub total_rows: u64,
    #[prost(uint64, tag = "10")]
    pub total_vectors: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbColumnDef {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub data_type: String,
    #[prost(bool, tag = "3")]
    pub is_primary: bool,
    #[prost(bool, tag = "4")]
    pub is_unique: bool,
    #[prost(bool, tag = "5")]
    pub is_not_null: bool,
    #[prost(bool, tag = "6")]
    pub auto_increment: bool,
    #[prost(string, optional, tag = "7")]
    pub default_value: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbTableSchema {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(message, repeated, tag = "2")]
    pub columns: Vec<PbColumnDef>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbRowItem {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(map = "string, string", tag = "2")]
    pub columns: HashMap<String, String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbRowChunk {
    #[prost(string, tag = "1")]
    pub table_name: String,
    #[prost(uint32, tag = "2")]
    pub chunk_index: u32,
    #[prost(message, repeated, tag = "3")]
    pub rows: Vec<PbRowItem>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbVectorPoint {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(float, repeated, tag = "2")]
    pub values: Vec<f32>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbVectorChunk {
    #[prost(string, tag = "1")]
    pub table_name: String,
    #[prost(string, tag = "2")]
    pub column_name: String,
    #[prost(uint32, tag = "3")]
    pub dimension: u32,
    #[prost(message, repeated, tag = "4")]
    pub vectors: Vec<PbVectorPoint>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbIndexDef {
    #[prost(string, tag = "1")]
    pub table_name: String,
    #[prost(string, repeated, tag = "2")]
    pub fields: Vec<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbGraphNode {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub label: String,
    #[prost(map = "string, string", tag = "3")]
    pub properties: HashMap<String, String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbGraphEdge {
    #[prost(string, tag = "1")]
    pub from_id: String,
    #[prost(string, tag = "2")]
    pub to_id: String,
    #[prost(string, tag = "3")]
    pub label: String,
    #[prost(map = "string, string", tag = "4")]
    pub properties: HashMap<String, String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbMemoryItem {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub memory_type: String,
    #[prost(string, tag = "3")]
    pub content: String,
    #[prost(uint64, tag = "4")]
    pub created_at: u64,
    #[prost(string, repeated, tag = "5")]
    pub tags: Vec<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbUser {
    #[prost(string, tag = "1")]
    pub username: String,
    #[prost(string, tag = "2")]
    pub password_hash: String,
    #[prost(string, tag = "3")]
    pub allowed_ip: String,
    #[prost(string, repeated, tag = "4")]
    pub roles: Vec<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbRole {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, repeated, tag = "2")]
    pub permissions: Vec<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbApiKey {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub key_hash: String,
    #[prost(string, tag = "3")]
    pub user_or_service: String,
    #[prost(string, repeated, tag = "4")]
    pub roles: Vec<String>,
    #[prost(bool, tag = "5")]
    pub is_revoked: bool,
    #[prost(uint64, tag = "6")]
    pub created_at: u64,
    #[prost(uint64, optional, tag = "7")]
    pub expires_at: Option<u64>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbTenant {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(uint64, tag = "3")]
    pub max_storage_bytes: u64,
    #[prost(uint64, tag = "4")]
    pub max_vectors: u64,
    #[prost(uint64, tag = "5")]
    pub max_qps: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbHistogramBucket {
    #[prost(string, tag = "1")]
    pub lower_bound: String,
    #[prost(string, tag = "2")]
    pub upper_bound: String,
    #[prost(uint64, tag = "3")]
    pub row_count: u64,
    #[prost(uint64, tag = "4")]
    pub distinct_count: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbColumnStats {
    #[prost(string, tag = "1")]
    pub column_name: String,
    #[prost(uint64, tag = "2")]
    pub null_count: u64,
    #[prost(uint64, tag = "3")]
    pub ndv: u64,
    #[prost(string, optional, tag = "4")]
    pub min_val: Option<String>,
    #[prost(string, optional, tag = "5")]
    pub max_val: Option<String>,
    #[prost(message, repeated, tag = "6")]
    pub buckets: Vec<PbHistogramBucket>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PbTableStats {
    #[prost(string, tag = "1")]
    pub table_name: String,
    #[prost(uint64, tag = "2")]
    pub total_rows: u64,
    #[prost(uint64, tag = "3")]
    pub analyzed_rows: u64,
    #[prost(uint64, tag = "4")]
    pub modification_counter: u64,
    #[prost(uint64, tag = "5")]
    pub last_analyzed: u64,
    #[prost(message, repeated, tag = "6")]
    pub columns: Vec<PbColumnStats>,
}

/// The top-level Protobuf Envelope container for native .pb snapshots
#[derive(Clone, PartialEq, Message)]
pub struct PbEnvelope {
    #[prost(message, optional, tag = "1")]
    pub header: Option<PbPackageHeader>,
    #[prost(message, repeated, tag = "2")]
    pub schemas: Vec<PbTableSchema>,
    #[prost(message, repeated, tag = "3")]
    pub row_chunks: Vec<PbRowChunk>,
    #[prost(message, repeated, tag = "4")]
    pub vector_chunks: Vec<PbVectorChunk>,
    #[prost(message, repeated, tag = "5")]
    pub indexes: Vec<PbIndexDef>,
    #[prost(message, repeated, tag = "6")]
    pub graph_nodes: Vec<PbGraphNode>,
    #[prost(message, repeated, tag = "7")]
    pub graph_edges: Vec<PbGraphEdge>,
    #[prost(message, repeated, tag = "8")]
    pub memories: Vec<PbMemoryItem>,
    #[prost(message, repeated, tag = "9")]
    pub users: Vec<PbUser>,
    #[prost(message, repeated, tag = "10")]
    pub roles: Vec<PbRole>,
    #[prost(message, repeated, tag = "11")]
    pub api_keys: Vec<PbApiKey>,
    #[prost(message, repeated, tag = "12")]
    pub tenants: Vec<PbTenant>,
    #[prost(message, repeated, tag = "13")]
    pub stats: Vec<PbTableStats>,
}
