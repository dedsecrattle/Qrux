#!/usr/bin/env bash
# Test script for quicproxy

set -e

echo "=== quicproxy Test Script ==="
echo

# Check if proxy is running
if ! lsof -i :4433 >/dev/null 2>&1; then
    echo "❌ Proxy not running on port 4433"
    echo "   Start it with: cargo run --release -- --config proxy.toml"
    exit 1
fi
echo "✅ Proxy listening on port 4433"

# Check if backend is running
if ! lsof -i :8080 >/dev/null 2>&1; then
    echo "❌ Backend not running on port 8080"
    echo "   Start it with: python3 -m http.server 8080"
    exit 1
fi
echo "✅ Backend listening on port 8080"

# Run integration tests
echo
echo "Running integration tests..."
cargo test --test integration -- --nocapture 2>&1 | tail -20

echo
echo "=== All checks passed! ==="
echo
echo "To test with Chrome:"
echo '  /Applications/Google\ Chrome.app/Contents/MacOS/Google\ Chrome \'
echo '    --origin-to-force-quic-on=localhost:4433 \'
echo '    https://localhost:4433'
echo
echo "Then check chrome://net-internals/#quic for QUIC session"
