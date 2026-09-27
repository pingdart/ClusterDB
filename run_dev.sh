#!/usr/bin/env bash
# ==========================================================
# PingDart Core DB - Auto-Restart Development Server
# ==========================================================

cd "$(dirname "$0")"

# Clean up any stale process holding the database lock or port 4650
pkill -f "target/debug/pingdart-core" 2>/dev/null || true
sleep 0.5

echo "⚡ Starting PingDart DB Dev Server with Auto-Restart..."
echo "📂 Watching: src/ (Rust files + dashboard.html), Cargo.toml"
echo "🌐 UI Available at: http://127.0.0.1:4650/database"
echo ""

# Prefer cargo watch if installed, fallback to nodemon
if command -v cargo-watch &>/dev/null; then
    cargo watch -w src -w Cargo.toml \
        -i "pingdart_data" \
        -i "backups" \
        -i "target" \
        -i "*.log" \
        -x "run --bin pingdart-core"
elif command -v nodemon &>/dev/null; then
    nodemon
else
    echo "Neither cargo-watch nor nodemon found. Installing cargo-watch..."
    cargo install cargo-watch
    cargo watch -w src -x "run --bin pingdart-core"
fi
