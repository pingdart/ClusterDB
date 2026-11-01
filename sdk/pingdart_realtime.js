/**
 * PingDart Realtime Client SDK (JavaScript / TypeScript)
 * Zero-dependency, Firebase-like Real-Time Database and Storage Client
 */
class PingDartClient {
  /**
   * @param {Object} options
   * @param {string} options.url - e.g. "http://localhost:4650"
   * @param {string} [options.token] - Auth JWT or API Key
   * @param {string} [options.database="default"] - Target Database
   */
  constructor(options = {}) {
    this.httpUrl = (options.url || "http://localhost:4650").replace(/\/$/, "");
    this.wsUrl = this.httpUrl.replace(/^http/, "ws") + "/v1/realtime";
    this.token = options.token || null;
    this.database = options.database || "default";
    this.ws = null;
    this.subscriptions = new Map(); // table -> Set of callbacks
    this.broadcastListeners = new Map(); // channel -> Set of callbacks
    this.presenceListeners = new Map(); // channel -> callback
    this.clientId = 'client_' + Math.random().toString(36).substring(2, 9);
    this.reconnectTimer = null;
    this.connected = false;
  }

  /** Connect to the Realtime WebSocket gateway */
  connect() {
    if (this.ws && (this.ws.readyState === WebSocket.OPEN || this.ws.readyState === WebSocket.CONNECTING)) {
      return Promise.resolve();
    }

    return new Promise((resolve, reject) => {
      const url = `${this.wsUrl}?client_id=${this.clientId}${this.token ? `&token=${this.token}` : ''}`;
      this.ws = new WebSocket(url);

      this.ws.onopen = () => {
        this.connected = true;
        // Resubscribe active tables
        this.subscriptions.forEach((_, table) => {
          this._send({
            action: "subscribe",
            table,
            database: this.database,
            fetch_snapshot: true
          });
        });
        resolve();
      };

      this.ws.onmessage = (event) => {
        try {
          const payload = JSON.parse(event.data);
          this._handleEvent(payload);
        } catch (e) {
          console.error("[PingDart] Malformed WS message:", e);
        }
      };

      this.ws.onclose = () => {
        this.connected = false;
        // Auto-reconnect after 2 seconds
        clearTimeout(this.reconnectTimer);
        this.reconnectTimer = setTimeout(() => this.connect(), 2000);
      };

      this.ws.onerror = (err) => {
        reject(err);
      };
    });
  }

  _send(obj) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(obj));
    }
  }

  _handleEvent(ev) {
    if (ev.type === "event" || ev.type === "snapshot") {
      const table = ev.table;
      const cbs = this.subscriptions.get(table);
      if (cbs) {
        cbs.forEach(cb => cb(ev));
      }
    } else if (ev.type === "broadcast") {
      const cbs = this.broadcastListeners.get(ev.channel);
      if (cbs) {
        cbs.forEach(cb => cb(ev.payload, ev.from_client));
      }
    } else if (ev.type === "presence_state" || ev.type === "presence_diff") {
      const cb = this.presenceListeners.get(ev.channel);
      if (cb) cb(ev);
    }
  }

  /**
   * Reference to a Collection / Table (Firebase Firestore-like)
   * @param {string} table
   */
  collection(table) {
    return new PingDartCollection(this, table);
  }

  /**
   * Storage client for uploading and downloading objects via HBSE
   */
  get storage() {
    return new PingDartStorage(this);
  }
}

class PingDartCollection {
  constructor(client, table) {
    this.client = client;
    this.table = table;
  }

  /**
   * Listen to all live real-time changes (INSERT, UPDATE, DELETE, SNAPSHOT)
   * @param {Function} callback - Called with event payload { type, event, record, old_record }
   * @returns {Function} Unsubscribe function
   */
  onSnapshot(callback) {
    if (!this.client.subscriptions.has(this.table)) {
      this.client.subscriptions.set(this.table, new Set());
      this.client.connect().then(() => {
        this.client._send({
          action: "subscribe",
          table: this.table,
          database: this.client.database,
          fetch_snapshot: true
        });
      });
    }

    this.client.subscriptions.get(this.table).add(callback);

    // Return unsubscription function
    return () => {
      const set = this.client.subscriptions.get(this.table);
      if (set) {
        set.delete(callback);
        if (set.size === 0) {
          this.client.subscriptions.delete(this.table);
          this.client._send({ action: "unsubscribe", table: this.table });
        }
      }
    };
  }

  /**
   * Insert record (automatically emits CDC event and notifies real-time listeners)
   * @param {Object} data
   */
  async add(data) {
    const res = await fetch(`${this.client.httpUrl}/v1/data/${this.table}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(data)
    });
    return res.json();
  }

  /**
   * Query records
   * @param {Object} [query={}] - { limit, offset, [field]: value }
   */
  async get(query = {}) {
    const params = new URLSearchParams(query);
    const res = await fetch(`${this.client.httpUrl}/v1/data/${this.table}?${params.toString()}`);
    return res.json();
  }

  /**
   * Update an existing record
   * @param {string} id
   * @param {Object} data
   */
  async update(id, data) {
    const res = await fetch(`${this.client.httpUrl}/v1/data/${this.table}/${id}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(data)
    });
    return res.json();
  }

  /**
   * Delete record
   * @param {string} id
   */
  async delete(id) {
    const res = await fetch(`${this.client.httpUrl}/v1/data/${this.table}/${id}`, {
      method: "DELETE"
    });
    return res.json();
  }
}

class PingDartStorage {
  constructor(client) {
    this.client = client;
  }

  /**
   * Upload file into HBSE binary storage and record metadata in PingDartDB
   * @param {File|Blob} file
   * @returns {Promise<Object>}
   */
  async upload(file) {
    const formData = new FormData();
    formData.append("file", file);

    const res = await fetch(`${this.client.httpUrl}/v1/storage/upload`, {
      method: "POST",
      body: formData
    });
    return res.json();
  }

  /** List uploaded storage objects */
  async list() {
    const res = await fetch(`${this.client.httpUrl}/v1/storage/files`);
    return res.json();
  }

  /** Get download URL for an object */
  getDownloadUrl(fileId) {
    return `${this.client.httpUrl}/v1/storage/files/${fileId}/download`;
  }
}

// Export for ES modules and Node
if (typeof module !== "undefined" && module.exports) {
  module.exports = { PingDartClient };
}
