use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::cdc::log::CDCRecord;
use crate::cdc::stream::CDCStreamer;
use crate::exec::RowData;
use crate::storage::StorageEngine;

/// Client subscription criteria
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSubscription {
    pub database: String,
    pub table: String,
    pub filter: Option<HashMap<String, String>>,
    pub primary_key: Option<String>,
}

/// Connected presence user state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceUser {
    pub user_id: String,
    pub client_id: String,
    pub channel: String,
    pub state: serde_json::Value,
    pub joined_at: i64,
}

/// Messages received from the client over WebSocket
#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ClientAction {
    Auth {
        token: String,
    },
    Subscribe {
        table: String,
        database: Option<String>,
        filter: Option<HashMap<String, String>>,
        primary_key: Option<String>,
        fetch_snapshot: Option<bool>,
    },
    Unsubscribe {
        table: String,
    },
    Broadcast {
        channel: String,
        payload: serde_json::Value,
    },
    PresenceTrack {
        channel: String,
        user_id: String,
        state: serde_json::Value,
    },
    PresenceLeave {
        channel: String,
    },
    Ping,
}

/// Messages sent from server to client over WebSocket
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    Connected {
        client_id: String,
        message: String,
        server_time: i64,
    },
    Authenticated {
        client_id: String,
        username: String,
    },
    Subscribed {
        table: String,
        database: String,
        filter: Option<HashMap<String, String>>,
    },
    Unsubscribed {
        table: String,
    },
    Snapshot {
        table: String,
        database: String,
        count: usize,
        records: Vec<serde_json::Value>,
    },
    Event {
        event: String, // "INSERT", "UPDATE", "DELETE", "CREATE_TABLE"
        database: String,
        table: String,
        primary_key: String,
        record: Option<serde_json::Value>,
        old_record: Option<serde_json::Value>,
        sequence_id: u64,
        timestamp: i64,
    },
    Broadcast {
        channel: String,
        payload: serde_json::Value,
        from_client: String,
        timestamp: i64,
    },
    PresenceState {
        channel: String,
        users: Vec<PresenceUser>,
    },
    PresenceDiff {
        channel: String,
        joins: Vec<PresenceUser>,
        leaves: Vec<String>,
    },
    Pong {
        timestamp: i64,
    },
    Error {
        message: String,
    },
}

struct ClientSession {
    #[allow(dead_code)]
    pub client_id: String,
    pub tx: mpsc::UnboundedSender<ServerEvent>,
    pub subscriptions: HashMap<String, TableSubscription>, // key = table
    pub presence_channels: HashMap<String, String>,        // channel -> user_id
    pub authenticated_user: Option<String>,
}

/// The centralized Realtime Hub powering Firebase-like live subscriptions and presence
#[derive(Clone)]
pub struct RealtimeHub {
    clients: Arc<RwLock<HashMap<String, ClientSession>>>,
    presence_rooms: Arc<RwLock<HashMap<String, HashMap<String, PresenceUser>>>>, // channel -> (user_id -> PresenceUser)
    #[allow(dead_code)]
    cdc_streamer: CDCStreamer,
    storage: StorageEngine,
}

impl RealtimeHub {
    pub fn new(cdc_streamer: CDCStreamer, storage: StorageEngine) -> Arc<Self> {
        let rx = cdc_streamer.subscribe();
        let hub = Arc::new(Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            presence_rooms: Arc::new(RwLock::new(HashMap::new())),
            cdc_streamer,
            storage,
        });

        // Spawn background task listening to the durable CDC commit log stream
        let hub_clone = hub.clone();
        tokio::spawn(async move {
            hub_clone.listen_cdc_events(rx).await;
        });

        hub
    }

    /// Background task that receives committed mutations from the CDC streamer
    async fn listen_cdc_events(&self, mut rx: tokio::sync::broadcast::Receiver<CDCRecord>) {
        info!("PingDart Realtime Hub CDC background listener active");

        while let Ok(record) = rx.recv().await {
            self.dispatch_cdc_record(record).await;
        }
        warn!("PingDart Realtime Hub CDC listener terminated");
    }

    /// Dispatches a CDCRecord to all matching subscribed WebSocket clients
    pub async fn dispatch_cdc_record(&self, record: CDCRecord) {
        let clients = self.clients.read().await;
        if clients.is_empty() {
            return;
        }

        let record_val = record.after_state_json();
        let old_val = record.before_state_json();

        let event = ServerEvent::Event {
            event: record.operation.clone(),
            database: record.database.clone(),
            table: record.table.clone(),
            primary_key: record.primary_key.clone(),
            record: record_val.clone(),
            old_record: old_val.clone(),
            sequence_id: record.sequence_id,
            timestamp: record.timestamp_ms,
        };

        for (_client_id, session) in clients.iter() {
            // Check if this client is subscribed to this table
            if let Some(sub) = session.subscriptions.get(&record.table) {
                // If sub has database specified, check it (or if default)
                let sub_db = if sub.database.is_empty() {
                    "default"
                } else {
                    sub.database.as_str()
                };
                let rec_db = if record.database.is_empty() {
                    "default"
                } else {
                    record.database.as_str()
                };

                let clean_rec_db = rec_db.trim_end_matches('_');
                if sub_db != clean_rec_db && sub_db != "%" {
                    continue;
                }

                // If sub filters by primary_key
                if let Some(ref pk) = sub.primary_key {
                    if pk != &record.primary_key {
                        continue;
                    }
                }

                // If sub has field filters
                if let Some(ref filters) = sub.filter {
                    if let Some(ref val_obj) = record_val {
                        let mut matches = true;
                        if let Some(map) = val_obj.as_object() {
                            for (field, target_val) in filters {
                                if let Some(field_val) = map.get(field) {
                                    let str_val = field_val
                                        .as_str()
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| field_val.to_string());
                                    if &str_val != target_val {
                                        matches = false;
                                        break;
                                    }
                                } else {
                                    matches = false;
                                    break;
                                }
                            }
                        }
                        if !matches {
                            continue;
                        }
                    }
                }

                // Match confirmed! Send real-time event to client
                let _ = session.tx.send(event.clone());
            }
        }
    }

    /// Registers a new WebSocket connection
    pub async fn register_client(&self, client_id: &str, tx: mpsc::UnboundedSender<ServerEvent>) {
        let mut clients = self.clients.write().await;
        clients.insert(
            client_id.to_string(),
            ClientSession {
                client_id: client_id.to_string(),
                tx,
                subscriptions: HashMap::new(),
                presence_channels: HashMap::new(),
                authenticated_user: None,
            },
        );
        debug!("Realtime client connected: {}", client_id);
    }

    /// Cleans up client session and removes presence across all joined rooms
    pub async fn unregister_client(&self, client_id: &str) {
        let removed = {
            let mut clients = self.clients.write().await;
            clients.remove(client_id)
        };

        if let Some(session) = removed {
            // Clean up presence in rooms
            let mut rooms = self.presence_rooms.write().await;
            for (channel, user_id) in session.presence_channels {
                if let Some(room) = rooms.get_mut(&channel) {
                    room.remove(&user_id);
                    // Broadcast presence leave to other subscribers
                    let diff = ServerEvent::PresenceDiff {
                        channel: channel.clone(),
                        joins: vec![],
                        leaves: vec![user_id],
                    };
                    self.broadcast_to_channel_peers(&channel, diff, client_id)
                        .await;
                }
            }
        }
        debug!("Realtime client disconnected: {}", client_id);
    }

    /// Sets authenticated user on client session
    pub async fn set_authenticated_user(&self, client_id: &str, username: &str) {
        let mut clients = self.clients.write().await;
        if let Some(session) = clients.get_mut(client_id) {
            session.authenticated_user = Some(username.to_string());
            let _ = session.tx.send(ServerEvent::Authenticated {
                client_id: client_id.to_string(),
                username: username.to_string(),
            });
        }
    }

    /// Subscribes client to a table/collection
    pub async fn subscribe(
        &self,
        client_id: &str,
        table: String,
        database: Option<String>,
        filter: Option<HashMap<String, String>>,
        primary_key: Option<String>,
        fetch_snapshot: bool,
    ) {
        let db = database.unwrap_or_else(|| "default".to_string());
        let sub = TableSubscription {
            database: db.clone(),
            table: table.clone(),
            filter: filter.clone(),
            primary_key: primary_key.clone(),
        };

        let tx = {
            let mut clients = self.clients.write().await;
            if let Some(session) = clients.get_mut(client_id) {
                session.subscriptions.insert(table.clone(), sub);
                Some(session.tx.clone())
            } else {
                None
            }
        };

        if let Some(tx) = tx {
            let _ = tx.send(ServerEvent::Subscribed {
                table: table.clone(),
                database: db.clone(),
                filter: filter.clone(),
            });

            // If client requested initial snapshot, fetch current records
            if fetch_snapshot {
                let records = self.fetch_table_snapshot(&db, &table, &filter, &primary_key);
                let _ = tx.send(ServerEvent::Snapshot {
                    table,
                    database: db,
                    count: records.len(),
                    records,
                });
            }
        }
    }

    /// Unsubscribes client from a table
    pub async fn unsubscribe(&self, client_id: &str, table: &str) {
        let tx = {
            let mut clients = self.clients.write().await;
            if let Some(session) = clients.get_mut(client_id) {
                session.subscriptions.remove(table);
                Some(session.tx.clone())
            } else {
                None
            }
        };

        if let Some(tx) = tx {
            let _ = tx.send(ServerEvent::Unsubscribed {
                table: table.to_string(),
            });
        }
    }

    /// Peer-to-peer or room broadcast without database persistence
    pub async fn broadcast(&self, from_client_id: &str, channel: &str, payload: serde_json::Value) {
        let clients = self.clients.read().await;
        let event = ServerEvent::Broadcast {
            channel: channel.to_string(),
            payload,
            from_client: from_client_id.to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
        };

        for (client_id, session) in clients.iter() {
            if client_id != from_client_id {
                let _ = session.tx.send(event.clone());
            }
        }
    }

    /// Tracks presence for a user in a channel
    pub async fn track_presence(
        &self,
        client_id: &str,
        channel: &str,
        user_id: &str,
        state: serde_json::Value,
    ) {
        let user = PresenceUser {
            user_id: user_id.to_string(),
            client_id: client_id.to_string(),
            channel: channel.to_string(),
            state,
            joined_at: chrono::Utc::now().timestamp_millis(),
        };

        // Record on client session
        {
            let mut clients = self.clients.write().await;
            if let Some(session) = clients.get_mut(client_id) {
                session
                    .presence_channels
                    .insert(channel.to_string(), user_id.to_string());
            }
        }

        // Record in room
        let all_users = {
            let mut rooms = self.presence_rooms.write().await;
            let room = rooms.entry(channel.to_string()).or_default();
            room.insert(user_id.to_string(), user.clone());
            room.values().cloned().collect::<Vec<_>>()
        };

        // Send full presence state to joining client
        {
            let clients = self.clients.read().await;
            if let Some(session) = clients.get(client_id) {
                let _ = session.tx.send(ServerEvent::PresenceState {
                    channel: channel.to_string(),
                    users: all_users,
                });
            }
        }

        // Broadcast join diff to peers
        let diff = ServerEvent::PresenceDiff {
            channel: channel.to_string(),
            joins: vec![user],
            leaves: vec![],
        };
        self.broadcast_to_channel_peers(channel, diff, client_id)
            .await;
    }

    /// Removes presence for a user from a channel
    pub async fn leave_presence(&self, client_id: &str, channel: &str) {
        let user_id = {
            let mut clients = self.clients.write().await;
            if let Some(session) = clients.get_mut(client_id) {
                session.presence_channels.remove(channel)
            } else {
                None
            }
        };

        if let Some(uid) = user_id {
            {
                let mut rooms = self.presence_rooms.write().await;
                if let Some(room) = rooms.get_mut(channel) {
                    room.remove(&uid);
                }
            }

            let diff = ServerEvent::PresenceDiff {
                channel: channel.to_string(),
                joins: vec![],
                leaves: vec![uid],
            };
            self.broadcast_to_channel_peers(channel, diff, client_id)
                .await;
        }
    }

    /// Internal helper: sends event to all clients except `except_client_id`
    async fn broadcast_to_channel_peers(
        &self,
        _channel: &str,
        event: ServerEvent,
        except_client_id: &str,
    ) {
        let clients = self.clients.read().await;
        for (client_id, session) in clients.iter() {
            if client_id != except_client_id {
                let _ = session.tx.send(event.clone());
            }
        }
    }

    /// Queries the Sled table tree to build the initial snapshot for a new subscriber
    fn fetch_table_snapshot(
        &self,
        database: &str,
        table: &str,
        filter: &Option<HashMap<String, String>>,
        primary_key: &Option<String>,
    ) -> Vec<serde_json::Value> {
        let full_table_name = if table.starts_with(&format!("{}_", database)) {
            table.to_string()
        } else {
            format!("{}_{}", database, table)
        };

        let mut results = Vec::new();

        if let Ok(tree) = self.storage.db().open_tree(&full_table_name) {
            for item in tree.iter() {
                if let Ok((key_bytes, val_bytes)) = item {
                    let key_str = String::from_utf8_lossy(&key_bytes).to_string();

                    // If pk filter given
                    if let Some(pk) = primary_key {
                        if pk != &key_str {
                            continue;
                        }
                    }

                    if let Ok(row) = RowData::try_from_bytes(&val_bytes) {
                        let mut map = serde_json::Map::new();
                        map.insert(
                            "id".to_string(),
                            serde_json::Value::String(if row._id.is_empty() {
                                key_str.clone()
                            } else {
                                row._id
                            }),
                        );
                        for (k, v) in row.columns {
                            // If it's valid JSON, keep as JSON value, else string
                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&v) {
                                map.insert(k, parsed);
                            } else {
                                map.insert(k, serde_json::Value::String(v));
                            }
                        }

                        // Check filters
                        if let Some(filters) = filter {
                            let mut matches = true;
                            for (fk, fv) in filters {
                                if let Some(val) = map.get(fk) {
                                    let s = val
                                        .as_str()
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| val.to_string());
                                    if &s != fv {
                                        matches = false;
                                        break;
                                    }
                                } else {
                                    matches = false;
                                    break;
                                }
                            }
                            if !matches {
                                continue;
                            }
                        }

                        results.push(serde_json::Value::Object(map));
                        if results.len() >= 500 {
                            break; // Cap snapshot limit
                        }
                    }
                }
            }
        }

        results
    }
}

/// Axum WebSocket upgrade handler for `/v1/realtime` and `/ws`
pub async fn handle_realtime_ws(
    ws: WebSocketUpgrade,
    State(hub): State<Arc<RealtimeHub>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let client_id = params
        .get("client_id")
        .cloned()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let token = params.get("token").cloned();

    ws.on_upgrade(move |socket| handle_socket(socket, hub, client_id, token))
}

/// Bi-directional async message loop for a single client connection
async fn handle_socket(
    socket: WebSocket,
    hub: Arc<RealtimeHub>,
    client_id: String,
    initial_token: Option<String>,
) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerEvent>();

    hub.register_client(&client_id, tx.clone()).await;

    // Send initial connected banner
    let now = chrono::Utc::now().timestamp_millis();
    let _ = tx.send(ServerEvent::Connected {
        client_id: client_id.clone(),
        message: "Connected to PingDart Cloud Real-Time Gateway".to_string(),
        server_time: now,
    });

    if let Some(token) = initial_token {
        hub.set_authenticated_user(&client_id, &token).await;
    }

    // Spawn outbound sender task
    let mut send_task = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let Ok(json_str) = serde_json::to_string(&event) {
                if ws_sender.send(Message::Text(json_str)).await.is_err() {
                    break;
                }
            }
        }
    });

    // Inbound receiver loop
    let hub_clone = hub.clone();
    let cid = client_id.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            match msg {
                Message::Text(text) => match serde_json::from_str::<ClientAction>(&text) {
                    Ok(action) => match action {
                        ClientAction::Auth { token } => {
                            hub_clone.set_authenticated_user(&cid, &token).await;
                        }
                        ClientAction::Subscribe {
                            table,
                            database,
                            filter,
                            primary_key,
                            fetch_snapshot,
                        } => {
                            hub_clone
                                .subscribe(
                                    &cid,
                                    table,
                                    database,
                                    filter,
                                    primary_key,
                                    fetch_snapshot.unwrap_or(true),
                                )
                                .await;
                        }
                        ClientAction::Unsubscribe { table } => {
                            hub_clone.unsubscribe(&cid, &table).await;
                        }
                        ClientAction::Broadcast { channel, payload } => {
                            hub_clone.broadcast(&cid, &channel, payload).await;
                        }
                        ClientAction::PresenceTrack {
                            channel,
                            user_id,
                            state,
                        } => {
                            hub_clone
                                .track_presence(&cid, &channel, &user_id, state)
                                .await;
                        }
                        ClientAction::PresenceLeave { channel } => {
                            hub_clone.leave_presence(&cid, &channel).await;
                        }
                        ClientAction::Ping => {
                            let _ = tx.send(ServerEvent::Pong {
                                timestamp: chrono::Utc::now().timestamp_millis(),
                            });
                        }
                    },
                    Err(e) => {
                        let _ = tx.send(ServerEvent::Error {
                            message: format!("Malformed command: {}", e),
                        });
                    }
                },
                Message::Ping(_p) => {
                    let _ = ws_sender_ping(&tx);
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // Wait until either task completes, then disconnect cleanly
    tokio::select! {
        _ = (&mut send_task) => {},
        _ = (&mut recv_task) => {},
    }

    send_task.abort();
    recv_task.abort();
    hub.unregister_client(&client_id).await;
}

fn ws_sender_ping(tx: &mpsc::UnboundedSender<ServerEvent>) {
    let _ = tx.send(ServerEvent::Pong {
        timestamp: chrono::Utc::now().timestamp_millis(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdc::log::CDCLog;
    use crate::storage::StorageEngine;

    #[tokio::test]
    async fn test_realtime_hub_subscription_and_cdc_dispatch() {
        let test_db_path = "test_realtime_hub_db";
        let storage = StorageEngine::new(test_db_path).unwrap();
        let cdc_log = CDCLog::new(storage.clone()).unwrap();
        let cdc_streamer = CDCStreamer::new(cdc_log);
        let hub = RealtimeHub::new(cdc_streamer.clone(), storage.clone());

        // Connect client
        let (tx, mut rx) = mpsc::unbounded_channel();
        let client_id = "test_client_1";
        hub.register_client(client_id, tx).await;

        // Subscribe to "users" table
        hub.subscribe(
            client_id,
            "users".to_string(),
            Some("default".to_string()),
            None,
            None,
            false,
        ).await;

        // Expect Subscribed event
        let sub_event = rx.recv().await.expect("Expected Subscribed event");
        match sub_event {
            ServerEvent::Subscribed { table, database, .. } => {
                assert_eq!(table, "users");
                assert_eq!(database, "default");
            }
            other => panic!("Unexpected event: {:?}", other),
        }

        // Emit CDC mutation to "users" table
        let record_val = serde_json::json!({ "id": "u1", "name": "Alice", "role": "admin" });
        cdc_streamer.emit(
            "default",
            "users",
            "INSERT",
            "u1",
            None,
            Some(record_val.clone()),
        ).unwrap();

        // Wait for dispatched event from background CDC listener
        let cdc_event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("Timed out waiting for CDC event")
            .expect("Expected Event");

        match cdc_event {
            ServerEvent::Event { event, table, primary_key, record, .. } => {
                assert_eq!(event, "INSERT");
                assert_eq!(table, "users");
                assert_eq!(primary_key, "u1");
                assert_eq!(record, Some(record_val));
            }
            other => panic!("Unexpected event: {:?}", other),
        }

        hub.unregister_client(client_id).await;
        let _ = std::fs::remove_dir_all(test_db_path);
    }

    #[tokio::test]
    async fn test_realtime_broadcast_and_presence() {
        let test_db_path = "test_realtime_broadcast_db";
        let storage = StorageEngine::new(test_db_path).unwrap();
        let cdc_log = CDCLog::new(storage.clone()).unwrap();
        let cdc_streamer = CDCStreamer::new(cdc_log);
        let hub = RealtimeHub::new(cdc_streamer, storage);

        let (tx1, _rx1) = mpsc::unbounded_channel();
        let (tx2, mut rx2) = mpsc::unbounded_channel();

        hub.register_client("client_1", tx1).await;
        hub.register_client("client_2", tx2).await;

        // Client 1 broadcasts to channel "chat"
        let msg = serde_json::json!({ "text": "Hello world!" });
        hub.broadcast("client_1", "chat", msg.clone()).await;

        // Client 2 should receive broadcast
        let bcast_event = rx2.recv().await.expect("Expected broadcast");
        match bcast_event {
            ServerEvent::Broadcast { channel, payload, from_client, .. } => {
                assert_eq!(channel, "chat");
                assert_eq!(payload, msg);
                assert_eq!(from_client, "client_1");
            }
            other => panic!("Unexpected event: {:?}", other),
        }

        // Client 1 tracks presence in "chat" room
        hub.track_presence(
            "client_1",
            "chat",
            "user_alice",
            serde_json::json!({ "status": "online" }),
        ).await;

        // Client 2 should receive presence diff notification
        let presence_event = rx2.recv().await.expect("Expected presence event");
        match presence_event {
            ServerEvent::PresenceDiff { channel, joins, .. } => {
                assert_eq!(channel, "chat");
                assert_eq!(joins.len(), 1);
                assert_eq!(joins[0].user_id, "user_alice");
            }
            other => panic!("Unexpected event: {:?}", other),
        }

        hub.unregister_client("client_1").await;
        hub.unregister_client("client_2").await;
        let _ = std::fs::remove_dir_all(test_db_path);
    }
}
