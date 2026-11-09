# How to Run the Leptos Web App Locally

## Prerequisites
1. **Trunk**: A WASM web application bundler for Rust.
   ```bash
   cargo install trunk
   ```
2. **WASM Target**: Add the WebAssembly target to Rust.
   ```bash
   rustup target add wasm32-unknown-unknown
   ```

## Running the App
Run the local development server with hot-reloading:
```bash
trunk serve --open
```

This will automatically build the WASM binary and open `http://localhost:8080` in your browser.

## Features Added
- Pure black/white OLED-friendly aesthetic (true black `#000` background).
- Reactive inputs using Leptos signals.
- Live, real-time price comparison calculation.
