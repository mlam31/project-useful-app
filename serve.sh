#!/bin/sh
# Serve the app on your LAN so you can open it from your iPhone.
# Uses the rustup shims first because the Homebrew cargo cannot add
# the wasm32-unknown-unknown target.
PATH="/opt/homebrew/opt/rustup/bin:$PATH"
exec trunk serve --address 0.0.0.0 --port 8080
