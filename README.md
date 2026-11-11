# Price Compare

Compare fruit & vegetable prices across stores — normalize everything to a
**price per kg** and instantly see which store is the best deal.

Built in **Rust** (Leptos + WebAssembly) as a **PWA**: open it in Safari on your
iPhone and "Add to Home Screen" for a fullscreen, app-like experience. Pure
black & white UI designed for OLED screens.

## What it does

The app is a small **database**: Stores → Products → Prices.

- **🏪 Stores tab** — every store you add (with optional address, pinnable via
  📍 GPS). Open a store to see all products recorded there, filter them, and
  **add prices**: product name (autocomplete searches your existing database),
  category (fruit / vegetable / bottle / other), **total price**,
  **quantity** (6-pack = 6), weight of one unit (g/kg/oz/lb, optional),
  **date** you found it and a **promo** flag — prices are temporary.
- **🔍 Products tab** — searchable list of every product in your database,
  each showing its current best price. Open a product to compare it across
  all stores: everything is normalized to a **price per kg** (derived from
  `price ÷ quantity × unit weight`) or per unit, cheapest first, winner
  highlighted, savings per kg shown. Only the most recent price per store
  competes, so old records don't distort the comparison.

All data is stored locally on your device (localStorage). Nothing is sent
anywhere, except the optional 📍 address lookup, which uses OpenStreetMap's
Nominatim reverse geocoding (© OpenStreetMap contributors).

## Run it locally

Prerequisites (one-time):

```bash
brew install trunk
rustup target add wasm32-unknown-unknown
```

> On this Mac the active `cargo` comes from Homebrew, which cannot add the
> WASM target. `serve.sh` fixes that by putting the rustup shims first on PATH.

Then:

```bash
./serve.sh
```

This starts a dev server with hot-reload on `http://localhost:8080`.

### Open it on your iPhone (same Wi-Fi)

1. Find your Mac's LAN IP: `ipconfig getifaddr en0`
2. On the iPhone, open Safari at `http://<mac-ip>:8080`
3. Share → **Add to Home Screen**

It will launch fullscreen with the black app icon, like a native app.

### Offline support (optional)

The service worker (offline caching) only registers over HTTPS or localhost.
Over plain HTTP on your LAN the app works fully, just without offline cache.
If you want offline mode, serve over HTTPS, e.g. with `mkcert`:

```bash
brew install mkcert && mkcert -install
mkcert -cert-file cert.pem -key-file key.pem 192.168.x.x
PATH="/opt/homebrew/opt/rustup/bin:$PATH" trunk serve --address 0.0.0.0 --port 8443 \
  --protocol https --cert-path cert.pem --key-file key.pem
```

(You will need to accept the self-signed warning once in Safari, and keep your
Mac reachable. No Apple Developer account is needed — this is a website.)

## Tests

```bash
cargo test
```

## Receipt scanning — possible?

Yes, with caveats. A PWA can open the iPhone camera and OCR can run entirely
in the browser (e.g. Tesseract.js compiled to WASM). Grocery receipts even
contain the exact data we need for weighed produce (`0.42 kg × 4.99/kg`),
so parsing can prefill store, per-kg price and weight. The limitations:
OCR accuracy on small thermal receipts is mediocre, formats differ per store,
and iOS Vision framework OCR is only available to native apps. It's a good v2
feature: "photo → prefill fields → you confirm", with manual entry staying the
fallback.

## Architecture

```
src/
  domain.rs   — pure logic: Store / Product / PriceEntry, normalization,
                latest-per-store rule, ranking (unit-tested)
  storage.rs  — localStorage persistence (stores, products, entries)
  main.rs     — Leptos UI: Stores / Store / Products / Product views + tab bar
  lib.rs      — exposes domain for integration tests
public/       — PWA manifest, service worker, app icons
index.html    — shell + all styling (black & white, OLED)
```
