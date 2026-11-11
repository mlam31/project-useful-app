# Product Tracker

Track and compare product prices across stores — normalize everything to a
**price per kg** (or per unit) and instantly see which store is the best deal.
Multi-currency: record prices in any currency while travelling, compare in
your favorite one.

Built in **Rust** (Leptos + WebAssembly) as a **PWA**: open it in Safari on your
iPhone and "Add to Home Screen" for a fullscreen, app-like experience. Pure
black & white UI designed for OLED screens.

## What it does

The app is a small **database**: Stores → Products → Prices, shown in three
bottom tabs (🏪 Stores / 🔍 Products / ⚙️ Settings).

- **🏪 Stores tab** — every store you add (with optional address). Open a store
  to see all products recorded there, filter them, and **add products**. The
  *"＋ Add a product"* form stays collapsed to a small bar until you tap it —
  same for *"＋ Add a store"*, which asks for the name after you tap. The form
  supports: product name (autocomplete searches your existing database +
  **auto-guesses the category** from the name, emoji shown per category),
  category override, **total price**, **quantity with unit** — pieces
  (5-for-10 = 5 pieces), or **grams / kg / liters / mL** ("500 g of mince",
  "1.5 L of milk"), optional weight of one piece (needed to compare per-kg
  offers against per-unit ones), **currency**, date and a **promo** flag —
  prices are temporary. Deleting a store is tucked away behind the **⋯ menu**.
- **🔍 Products tab** — searchable list of every product in your database,
  filterable by **category chips** (All / 🍎 Fruit / 🥬 Veggies / 🧴 Drinks /
  📦 Other), each showing its current best price. **Tick the checkboxes** to
  select similar-but-different products (green kiwi + yellow kiwi) and tap
  **"⚖️ Compare selected"** to rank offers across all of them. Open a product
  to compare it across all stores: everything is normalized to a **price per
  kg** (derived from `price ÷ quantity × unit weight`), **per liter** (for
  quantities entered in L/mL) or per unit, cheapest first, winner highlighted,
  savings shown, store names link to Apple Maps. Only the most recent price
  per (store, product) competes, so old records don't distort the comparison.
  Deleting a product is tucked away behind the **⋯ menu**.
- **⚙️ Settings tab** — pick your **main currency** (favorite: EUR, USD, AUD,
  JPY, RMB…). All comparisons are shown in it. Optionally set a **second
  currency**: converted prices are shown right next to the main ones — handy
  to keep a feel for prices from back home (e.g. see EUR while shopping in
  AUD). Prices are recorded in whatever currency you were paying with — old
  prices keep their original currency and are **converted automatically**
  (original values shown in small print when they differ). Exchange rates can
  be edited manually, refreshed live (free ECB API, Frankfurter) with one tap,
  and you can **add any new currency** (code + symbol + rate).

All data is stored locally on your device (localStorage). Nothing is sent
anywhere, except live-rate fetches, the optional 📍 address lookup
(OpenStreetMap Nominatim, © OpenStreetMap contributors) and the maps links.

> Note on 📍: Safari only provides GPS on **HTTPS** pages, so pinning the
> address works once deployed over HTTPS (or on localhost), not over plain
> HTTP on the LAN. Exact place-picking like Google Maps "QV Market" would
> require a paid Google Places API key — the free geocoder returns an address
> from your coordinates instead.

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
  domain.rs   — pure logic: Store / Product / PriceEntry / Currency / Settings,
                normalization, latest-per-store rule, category guessing,
                default currencies (unit-tested)
  storage.rs  — localStorage persistence (stores, products, entries,
                currencies, settings)
  main.rs     — Leptos UI: tab bar, Stores / Store / Products / Product /
                Compare / Settings views, ranking & comparison list
  lib.rs      — exposes domain for integration tests
public/       — PWA manifest, service worker, app icons
index.html    — shell + all styling (black & white, OLED)
```
