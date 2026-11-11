//! Price Compare — a small database of stores → products → prices,
//! with fair per-kg / per-unit comparison across stores and multi-currency
//! support. Black & white OLED-friendly PWA built with Leptos (CSR).

mod storage;

use leptos::prelude::*;
use leptos::task::spawn_local;
use useful_app::domain::{self, Currency, PriceEntry, Product, Settings, Store};
use wasm_bindgen::prelude::*;
use web_sys::PositionOptions;

// ---------------------------------------------------------------- helpers

/// Today's date (device local time) as YYYY-MM-DD.
fn today_iso() -> String {
    let d = js_sys::Date::new(&js_sys::Date::now().into());
    format!(
        "{:04}-{:02}-{:02}",
        d.get_full_year(),
        d.get_month() + 1,
        d.get_date()
    )
}

fn fmt_price(x: f64) -> String {
    if x > 0.0 && x < 0.1 {
        format!("{:.3}", x)
    } else {
        format!("{:.2}", x)
    }
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn next_id(ids: impl Iterator<Item = u64>) -> u64 {
    ids.max().unwrap_or(0) + 1
}

fn update_store(stores: RwSignal<Vec<Store>>, id: u64, f: impl FnOnce(&mut Store)) {
    stores.update(|v| {
        if let Some(s) = v.iter_mut().find(|s| s.id == id) {
            f(s);
        }
    });
}

fn symbol_of(currencies: &[Currency], code: &str) -> String {
    currencies
        .iter()
        .find(|c| c.code == code)
        .map(|c| c.symbol.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| domain::currency_symbol(code).to_string())
}

/// Main-currency units per 1 unit of `code` (1.0 when unknown → treat as main).
fn currency_rate(currencies: &[Currency], code: &str) -> f64 {
    currencies
        .iter()
        .find(|c| c.code == code)
        .map(|c| c.rate)
        .filter(|r| *r > 0.0 && r.is_finite())
        .unwrap_or(1.0)
}

// ---------------------------------------------------------------- navigation

#[derive(Clone, PartialEq)]
enum Nav {
    Stores,
    StoreDetail(u64),
    Products,
    ProductDetail(u64),
    Compare(Vec<u64>),
    Settings,
}

// ---------------------------------------------------------------- ranking

#[derive(Clone, Debug, PartialEq)]
struct RankRow {
    entry_id: u64,
    store_id: u64,
    store_name: String,
    product_id: u64,
    product_name: String,
    product_emoji: String,
    promo: bool,
    date: String,
    /// Converted to the main currency.
    per_unit: Option<f64>,
    per_kg: Option<f64>,
    /// The value used for ranking: per-kg when any entry has a weight, else per-unit.
    metric: Option<f64>,
    delta: Option<f64>,
    best: bool,
    kg_mode: bool,
    /// Original values in the recorded currency (when different from main).
    orig_text: Option<String>,
}

/// Rank the prices of one or several products across stores.
/// Only the most recent entry per (store, product) competes; everything is
/// converted to the main currency; entries without a comparable metric go last.
#[allow(clippy::too_many_arguments)]
fn compute_ranking(
    entries: &[PriceEntry],
    stores: &[Store],
    products: &[Product],
    currencies: &[Currency],
    main_code: &str,
    ids: &[u64],
) -> Vec<RankRow> {
    let filtered: Vec<PriceEntry> = entries
        .iter()
        .filter(|e| ids.contains(&e.product_id))
        .cloned()
        .collect();
    let latest = domain::latest_per_store(&filtered);

    let mut rows: Vec<RankRow> = latest
        .iter()
        .map(|e| {
            let q = domain::entry_quote(e);
            let product = products.iter().find(|p| p.id == e.product_id);
            let product_name = product
                .map(|p| p.name.trim().to_string())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "Product".to_string());
            let product_emoji = product
                .map(|p| domain::category_emoji(&p.category).to_string())
                .unwrap_or_else(|| "📦".to_string());
            let store_name = stores
                .iter()
                .find(|s| s.id == e.store_id)
                .map(|s| s.name.trim().to_string())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("Store #{}", e.store_id));

            let cur = if e.currency.is_empty() {
                main_code.to_string()
            } else {
                e.currency.clone()
            };
            let rate = currency_rate(currencies, &cur);

            let orig_text = if cur != main_code {
                let sym = symbol_of(currencies, &cur);
                let mut parts = Vec::new();
                if let Some(k) = q.per_kg {
                    parts.push(format!("{}{} /kg", sym, fmt_price(k)));
                }
                if let Some(u) = q.per_unit {
                    parts.push(format!("{}{} each", sym, fmt_price(u)));
                }
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join(" · "))
                }
            } else {
                None
            };

            RankRow {
                entry_id: e.id,
                store_id: e.store_id,
                store_name,
                product_id: e.product_id,
                product_name,
                product_emoji,
                promo: e.promo,
                date: e.date.clone(),
                per_unit: q.per_unit.map(|v| v * rate),
                per_kg: q.per_kg.map(|v| v * rate),
                metric: None,
                delta: None,
                best: false,
                kg_mode: false,
                orig_text,
            }
        })
        .collect();

    let kg_mode = rows.iter().any(|r| r.per_kg.is_some());
    for r in rows.iter_mut() {
        r.kg_mode = kg_mode;
        r.metric = if kg_mode { r.per_kg } else { r.per_unit };
    }

    // Cheapest first; entries without a metric at the end.
    rows.sort_by(|a, b| match (a.metric, b.metric) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    let metrics: Vec<Option<f64>> = rows.iter().map(|r| r.metric).collect();
    let min = match domain::best_metric(&metrics) {
        domain::Best::Winner(i) | domain::Best::Tied(i) => rows[i].metric,
        domain::Best::None => None,
    };

    if let Some(min) = min {
        for r in rows.iter_mut() {
            if let Some(m) = r.metric {
                r.delta = Some(m - min);
                r.best = (m - min).abs() <= min * 1e-6;
            }
        }
    }

    rows
}

// ---------------------------------------------------------------- GPS pin

async fn reverse_geocode(lat: f64, lon: f64) -> String {
    let fallback = format!("{:.5}, {:.5}", lat, lon);
    let url = format!(
        "https://nominatim.openstreetmap.org/reverse?format=jsonv2&lat={lat}&lon={lon}&zoom=18&addressdetails=0"
    );
    match gloo_net::http::Request::get(&url).send().await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(v) => v
                .get("display_name")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
                .unwrap_or(fallback),
            Err(_) => fallback,
        },
        Err(_) => fallback,
    }
}

/// Ask the browser for the current position, reverse-geocode it and hand the
/// address to `apply`. Failures are reported to `on_error`.
fn pin_address(
    apply: impl Fn(String) + 'static,
    on_error: impl Fn(String) + 'static,
    pinning: RwSignal<bool>,
) {
    let Some(window) = web_sys::window() else {
        on_error("Browser location not available".to_string());
        return;
    };
    let Some(geo) = window.navigator().geolocation().ok() else {
        on_error("Browser location not available".to_string());
        return;
    };

    pinning.set(true);
    let opts = PositionOptions::new();
    opts.set_enable_high_accuracy(true);
    opts.set_timeout(10_000u32);

    let on_ok = Closure::once(move |pos: web_sys::Position| {
        let coords = pos.coords();
        let lat = coords.latitude();
        let lon = coords.longitude();
        spawn_local(async move {
            let addr = reverse_geocode(lat, lon).await;
            apply(addr);
            pinning.set(false);
        });
    });

    let on_err = Closure::once(move |e: web_sys::PositionError| {
        let msg = match e.code() as u32 {
            1 => "Location permission denied — allow it for this site in Safari settings. Note: Safari only provides GPS on HTTPS pages (this works once the app is deployed).",
            2 => "Location unavailable right now",
            3 => "Location timed out",
            _ => "Location failed",
        };
        on_error(msg.to_string());
        pinning.set(false);
    });

    let _ = geo.get_current_position_with_error_callback_and_options(
        on_ok.as_ref().unchecked_ref(),
        Some(on_err.as_ref().unchecked_ref()),
        &opts,
    );

    // The closures may fire after this frame; keep them alive.
    on_ok.forget();
    on_err.forget();
}

// ---------------------------------------------------------------- tab bar

#[component]
fn TabBar(nav: RwSignal<Nav>) -> impl IntoView {
    let stores_active = move || {
        matches!(nav.get(), Nav::Stores | Nav::StoreDetail(_) | Nav::Compare(_))
    };
    let products_active = move || {
        matches!(nav.get(), Nav::Products | Nav::ProductDetail(_) | Nav::Compare(_))
    };
    let settings_active = move || matches!(nav.get(), Nav::Settings);

    view! {
        <nav class="tabbar">
            <button
                class=move || if stores_active() { "tab active" } else { "tab" }
                on:click=move |_| nav.set(Nav::Stores)
            >
                "🏪 Stores"
            </button>
            <button
                class=move || if products_active() { "tab active" } else { "tab" }
                on:click=move |_| nav.set(Nav::Products)
            >
                "🔍 Products"
            </button>
            <button
                class=move || if settings_active() { "tab active" } else { "tab" }
                on:click=move |_| nav.set(Nav::Settings)
            >
                "⚙️ Settings"
            </button>
        </nav>
    }
}

// ---------------------------------------------------------------- stores view

#[component]
fn StoresView(
    stores: RwSignal<Vec<Store>>,
    entries: RwSignal<Vec<PriceEntry>>,
    nav: RwSignal<Nav>,
) -> impl IntoView {
    let search = RwSignal::new(String::new());
    let adding = RwSignal::new(false);
    let new_name = RwSignal::new(String::new());

    let filtered = Memo::new(move |_| {
        let q = search.get().trim().to_lowercase();
        let mut list: Vec<Store> = stores
            .get()
            .into_iter()
            .filter(|st| q.is_empty() || st.name.to_lowercase().contains(&q))
            .collect();
        list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        list
    });

    let add_store = move |_| {
        let name = new_name.get_untracked().trim().to_string();
        if name.is_empty() {
            return;
        }
        let id = next_id(stores.get_untracked().iter().map(|s| s.id));
        stores.update(|v| v.push(Store { id, name, address: None }));
        new_name.set(String::new());
        adding.set(false);
    };

    view! {
        <div class="page">
            <header><h1>"🏪 Stores"</h1></header>

            <input
                class="search-input"
                placeholder="Search stores"
                prop:value=move || search.get()
                on:input=move |ev| search.set(event_target_value(&ev))
            />

            <Show
                when=move || adding.get()
                fallback=move || {
                    view! {
                        <button class="collapse-btn" on:click=move |_| adding.set(true)>
                            "＋ Add a store"
                        </button>
                    }
                }
            >
                <div class="card">
                    <div class="row">
                        <input
                            class="store-input"
                            placeholder="Store name"
                            prop:value=move || new_name.get()
                            on:input=move |ev| new_name.set(event_target_value(&ev))
                        />
                        <button class="icon-btn" title="Cancel" on:click=move |_| {
                            adding.set(false);
                            new_name.set(String::new());
                        }>"✕"</button>
                    </div>
                    <button class="add-btn" on:click=add_store>"Create store"</button>
                </div>
            </Show>

            <Show when=move || stores.get().is_empty()>
                <div class="hint">
                    "Add your first store — then record the products you buy there."
                </div>
            </Show>

            <For
                each=move || filtered.get()
                key=|st| st.id
                children={move |st: Store| {
                    let sid = st.id;
                    view! {
                        <div class="store-row" on:click=move |_| nav.set(Nav::StoreDetail(sid))>
                            <div class="s-info">
                                <div class="s-name">{st.name.clone()}</div>
                                <div class="s-sub">
                                    {move || {
                                        let addr = stores.get().iter().find(|s| s.id == sid)
                                            .and_then(|s| s.address.clone())
                                            .filter(|a| !a.trim().is_empty());
                                        let n = entries.get().iter()
                                            .filter(|e| e.store_id == sid)
                                            .map(|e| e.product_id)
                                            .collect::<std::collections::HashSet<_>>()
                                            .len();
                                        let products_txt = match n {
                                            0 => "no products yet".to_string(),
                                            1 => "1 product".to_string(),
                                            n => format!("{} products", n),
                                        };
                                        match addr {
                                            Some(a) => format!("{} · {}", products_txt, a),
                                            None => products_txt,
                                        }
                                    }}
                                </div>
                            </div>
                            <div class="chev">"›"</div>
                        </div>
                    }
                }}
            />
        </div>
    }
}

// ---------------------------------------------------------------- store detail

#[component]
fn StoreView(
    store_id: u64,
    stores: RwSignal<Vec<Store>>,
    products: RwSignal<Vec<Product>>,
    entries: RwSignal<Vec<PriceEntry>>,
    currencies: RwSignal<Vec<Currency>>,
    settings: RwSignal<Settings>,
    nav: RwSignal<Nav>,
) -> impl IntoView {
    // ---- existing entries for this store
    let filter = RwSignal::new(String::new());

    let rows = Memo::new(move |_| {
        let q = filter.get().trim().to_lowercase();
        let prods = products.get();
        let mut list: Vec<(PriceEntry, String, String)> = entries
            .get()
            .iter()
            .filter(|e| e.store_id == store_id)
            .map(|e| {
                let p = prods.iter().find(|p| p.id == e.product_id);
                (
                    e.clone(),
                    p.map(|p| p.name.clone()).unwrap_or_else(|| "?".into()),
                    p.map(|p| p.category.clone()).unwrap_or_else(|| "other".into()),
                )
            })
            .filter(|(_, name, _)| q.is_empty() || name.to_lowercase().contains(&q))
            .collect();
        list.sort_by(|a, b| {
            a.1.to_lowercase()
                .cmp(&b.1.to_lowercase())
                .then(b.0.date.cmp(&a.0.date))
                .then(b.0.id.cmp(&a.0.id))
        });
        list
    });

    let delete_entry = move |entry_id: u64| {
        entries.update(|v| v.retain(|e| e.id != entry_id));
    };

    // ---- add price form (collapsed by default)
    let form_open = RwSignal::new(false);
    let new_product = RwSignal::new(String::new());
    let new_cat = RwSignal::new("other".to_string());
    let cat_touched = RwSignal::new(false);
    let new_price = RwSignal::new(String::new());
    let new_qty = RwSignal::new("1".to_string());
    let new_weight = RwSignal::new(String::new());
    let new_wunit = RwSignal::new("g".to_string());
    let new_cur = RwSignal::new(settings.get_untracked().main_currency.clone());
    let new_promo = RwSignal::new(false);
    let new_date = RwSignal::new(today_iso());

    let suggestions = Memo::new(move |_| {
        let t = new_product.get().trim().to_lowercase();
        if t.is_empty() {
            Vec::new()
        } else {
            products
                .get()
                .iter()
                .filter(|p| p.name.to_lowercase().contains(&t))
                .take(6)
                .cloned()
                .collect()
        }
    });

    // Category suggestion while typing (until the user picks one manually).
    let guessed = Memo::new(move |_| domain::guess_category(&new_product.get()).to_string());
    let current_cat = move || if cat_touched.get() { new_cat.get() } else { guessed.get() };

    let save_price = move |_| {
        let name = new_product.get_untracked().trim().to_string();
        if name.is_empty() {
            return;
        }

        // Find the product in the database, or create it.
        let pid = products
            .get_untracked()
            .iter()
            .find(|p| p.name.trim().eq_ignore_ascii_case(&name))
            .map(|p| p.id)
            .unwrap_or_else(|| {
                let id = next_id(products.get_untracked().iter().map(|p| p.id));
                let category = current_cat();
                products.update(|v| v.push(Product { id, name: name.clone(), category }));
                id
            });

        let eid = next_id(entries.get_untracked().iter().map(|e| e.id));
        let mut entry = PriceEntry::new(eid, store_id, pid);
        entry.price = new_price.get_untracked();
        let qty = new_qty.get_untracked();
        entry.quantity = if qty.trim().is_empty() { "1".to_string() } else { qty };
        entry.unit_weight = new_weight.get_untracked();
        entry.weight_unit = new_wunit.get_untracked();
        entry.currency = if new_cur.get_untracked().is_empty() {
            settings.get_untracked().main_currency.clone()
        } else {
            new_cur.get_untracked()
        };
        entry.promo = new_promo.get_untracked();
        entry.date = new_date.get_untracked();
        entries.update(|v| v.push(entry));

        // Reset the form (keep the date, it usually stays "today").
        new_product.set(String::new());
        new_cat.set("other".to_string());
        cat_touched.set(false);
        new_price.set(String::new());
        new_qty.set("1".to_string());
        new_weight.set(String::new());
        new_promo.set(false);
        form_open.set(false);
    };

    // ---- store header
    let pinning = RwSignal::new(false);
    let pin_error = RwSignal::new(String::new());
    let pin = move |_| {
        pin_error.set(String::new());
        pin_address(
            move |addr| update_store(stores, store_id, move |s| s.address = Some(addr)),
            move |msg| pin_error.set(msg),
            pinning,
        );
    };

    let delete_store = move |_| {
        stores.update(|v| v.retain(|s| s.id != store_id));
        entries.update(|v| v.retain(|e| e.store_id != store_id));
        nav.set(Nav::Stores);
    };

    view! {
        <div class="page">
            <button class="back-btn" on:click=move |_| nav.set(Nav::Stores)>"← Stores"</button>

            <input
                class="store-title"
                placeholder="Store name"
                prop:value=move || stores.get().iter().find(|s| s.id == store_id).map(|s| s.name.clone()).unwrap_or_default()
                on:input=move |ev| update_store(stores, store_id, move |s| s.name = event_target_value(&ev))
            />

            <div class="add-inline">
                <input
                    class="addr-input"
                    placeholder="Address (optional)"
                    prop:value=move || stores.get().iter().find(|s| s.id == store_id).and_then(|s| s.address.clone()).unwrap_or_default()
                    on:input=move |ev| update_store(stores, store_id, move |s| s.address = Some(event_target_value(&ev)))
                />
                <button class="icon-btn" title="Use my current location" on:click=pin>"📍"</button>
            </div>
            <Show when=move || pinning.get()>
                <div class="hint">"Locating…"</div>
            </Show>
            <Show when=move || !pin_error.get().is_empty()>
                <div class="hint error">{move || pin_error.get()}</div>
            </Show>

            // ---- recorded products
            <h2 class="section">"Products recorded here"</h2>
            <input
                class="search-input"
                placeholder="Filter products"
                prop:value=move || filter.get()
                on:input=move |ev| filter.set(event_target_value(&ev))
            />

            <Show when=move || rows.get().is_empty()>
                <div class="hint">"No prices recorded yet. Add the first one below."</div>
            </Show>

            <For
                each=move || rows.get()
                key=|row| row.0.id
                children={move |row: (PriceEntry, String, String)| {
                    let entry = row.0;
                    let eid = entry.id;
                    let quote = domain::entry_quote(&entry);
                    let emoji = domain::category_emoji(&row.2);
                    let qty = domain::parse_number(&entry.quantity).unwrap_or(1.0);
                    let cur = if entry.currency.is_empty() {
                        settings.get_untracked().main_currency.clone()
                    } else {
                        entry.currency.clone()
                    };
                    let sym = symbol_of(&currencies.get_untracked(), &cur);
                    let price_txt = if qty != 1.0 {
                        format!("{}{} × {}", sym, entry.price.trim(), qty)
                    } else {
                        format!("{}{}", sym, entry.price.trim())
                    };
                    let sub = match quote.per_kg {
                        Some(k) => format!(
                            "{} each · {} /kg",
                            fmt_price(quote.per_unit.unwrap_or(0.0)),
                            fmt_price(k)
                        ),
                        None => match quote.per_unit {
                            Some(u) => format!("{} each", fmt_price(u)),
                            None => "—".to_string(),
                        },
                    };
                    view! {
                        <div class="entry-row">
                            <div class="e-info">
                                <div class="e-name">{emoji}" "{row.1}</div>
                                <div class="e-meta">
                                    {entry.date.clone()}
                                    {move || {
                                        if entry.promo { Some(view! { <span class="promo-badge">"PROMO"</span> }) } else { None }
                                    }}
                                </div>
                            </div>
                            <div class="e-prices">
                                <div class="e-sub">{price_txt}</div>
                                <div class="e-per">{sub}</div>
                            </div>
                            <button class="icon-btn small" title="Delete this price" on:click=move |_| delete_entry(eid)>"✕"</button>
                        </div>
                    }
                }}
            />

            // ---- add price card (collapsed until tapped)
            <h2 class="section">"Add a price"</h2>
            <Show
                when=move || form_open.get()
                fallback=move || {
                    view! {
                        <button class="collapse-btn" on:click=move |_| form_open.set(true)>
                            "＋ Add a price"
                        </button>
                    }
                }
            >
                <div class="card">
                    <div class="row form-head">
                        <span class="form-title">"New price"</span>
                        <button class="icon-btn small" title="Collapse" on:click=move |_| form_open.set(false)>"—"</button>
                    </div>

                    <input
                        class="store-input"
                        placeholder="Product (e.g. Apples, Cucumber, Bottle of water)"
                        prop:value=move || new_product.get()
                        on:input=move |ev| new_product.set(event_target_value(&ev))
                    />
                    <Show when=move || !suggestions.get().is_empty()>
                        <div class="chips">
                            <For
                                each=move || suggestions.get()
                                key=|p| p.id
                                children={move |p: Product| {
                                    view! {
                                        <button class="chip" on:click=move |_| new_product.set(p.name.clone())>
                                            {domain::category_emoji(&p.category)}" "{p.name.clone()}
                                        </button>
                                    }
                                }}
                            />
                        </div>
                    </Show>

                    <div class="grid2">
                        <label class="field">
                            <span class="lbl">"Category"</span>
                            <select
                                class="unit-select wide"
                                prop:value=move || current_cat()
                                on:change=move |ev| {
                                    cat_touched.set(true);
                                    new_cat.set(event_target_value(&ev));
                                }
                            >
                                {move || {
                                    domain::CATEGORIES.iter().map(|(id, label)| {
                                        view! { <option value=*id>{*label}</option> }
                                    }).collect_view()
                                }}
                            </select>
                        </label>
                        <label class="field">
                            <span class="lbl">"Price (total)"</span>
                            <input
                                inputmode="decimal"
                                placeholder="5.00"
                                prop:value=move || new_price.get()
                                on:input=move |ev| new_price.set(event_target_value(&ev))
                            />
                        </label>
                    </div>

                    <div class="grid3">
                        <label class="field">
                            <span class="lbl">"Qty (units)"</span>
                            <input
                                inputmode="decimal"
                                placeholder="3"
                                prop:value=move || new_qty.get()
                                on:input=move |ev| new_qty.set(event_target_value(&ev))
                            />
                        </label>
                        <label class="field">
                            <span class="lbl">"1 unit (weight)"</span>
                            <input
                                inputmode="decimal"
                                placeholder="90"
                                prop:value=move || new_weight.get()
                                on:input=move |ev| new_weight.set(event_target_value(&ev))
                            />
                        </label>
                        <label class="field">
                            <span class="lbl">"Weight unit"</span>
                            <select
                                class="unit-select"
                                prop:value=move || new_wunit.get()
                                on:change=move |ev| new_wunit.set(event_target_value(&ev))
                            >
                                <option value="g">"g"</option>
                                <option value="kg">"kg"</option>
                                <option value="oz">"oz"</option>
                                <option value="lb">"lb"</option>
                            </select>
                        </label>
                    </div>

                    <div class="grid2">
                        <label class="field">
                            <span class="lbl">"Currency"</span>
                            <select
                                class="unit-select wide"
                                prop:value=move || new_cur.get()
                                on:change=move |ev| new_cur.set(event_target_value(&ev))
                            >
                                {move || {
                                    currencies.get().iter().map(|c: &Currency| {
                                        view! { <option value=c.code.clone()>{c.symbol.clone()}" "{c.code.clone()}</option> }
                                    }).collect_view()
                                }}
                            </select>
                        </label>
                        <label class="field">
                            <span class="lbl">"Found on"</span>
                            <input
                                type="date"
                                prop:value=move || new_date.get()
                                on:input=move |ev| new_date.set(event_target_value(&ev))
                            />
                        </label>
                    </div>

                    <div class="row meta-row">
                        <label class="check">
                            <input
                                type="checkbox"
                                prop:checked=move || new_promo.get()
                                on:change=move |ev| new_promo.set(event_target_checked(&ev))
                            />
                            <span>"Promo (temporary price)"</span>
                        </label>
                    </div>

                    <button class="add-btn" on:click=save_price>"Save price"</button>
                </div>
            </Show>

            <button class="clear-btn" on:click=delete_store>"Delete this store"</button>
        </div>
    }
}

// ---------------------------------------------------------------- products view

#[component]
fn ProductsView(
    products: RwSignal<Vec<Product>>,
    entries: RwSignal<Vec<PriceEntry>>,
    stores: RwSignal<Vec<Store>>,
    currencies: RwSignal<Vec<Currency>>,
    settings: RwSignal<Settings>,
    nav: RwSignal<Nav>,
) -> impl IntoView {
    let search = RwSignal::new(String::new());
    let cat_filter = RwSignal::new(String::new());
    let selected: RwSignal<Vec<u64>> = RwSignal::new(Vec::new());

    let filtered = Memo::new(move |_| {
        let q = search.get().trim().to_lowercase();
        let cat = cat_filter.get();
        let mut list: Vec<Product> = products
            .get()
            .into_iter()
            .filter(|p| q.is_empty() || p.name.to_lowercase().contains(&q))
            .filter(|p| cat.is_empty() || p.category == cat)
            .collect();
        list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        list
    });

    let summary_for = move |pid: u64| -> String {
        let rows = compute_ranking(
            &entries.get_untracked(),
            &stores.get_untracked(),
            &products.get_untracked(),
            &currencies.get_untracked(),
            &settings.get_untracked().main_currency,
            &[pid],
        );
        match rows.iter().find(|r| r.metric.is_some()) {
            Some(best) => {
                let m = best.metric.unwrap();
                let sym = symbol_of(&currencies.get_untracked(), &settings.get_untracked().main_currency);
                let unit = if best.kg_mode { "/kg" } else { "each" };
                format!("from {}{} {} · {}", sym, fmt_price(m), unit, best.store_name)
            }
            None => "no prices yet".to_string(),
        }
    };

    let toggle = move |pid: u64| {
        selected.update(|v| {
            if let Some(i) = v.iter().position(|&x| x == pid) {
                v.remove(i);
            } else {
                v.push(pid);
            }
        });
    };

    let compare_selected = move |_| {
        let ids = selected.get_untracked();
        if !ids.is_empty() {
            selected.set(Vec::new());
            nav.set(Nav::Compare(ids));
        }
    };

    view! {
        <div class="page">
            <header><h1>"🔍 Products"</h1></header>

            <input
                class="search-input"
                placeholder="Search the database (fruits, vegetables, bottles…)"
                prop:value=move || search.get()
                on:input=move |ev| search.set(event_target_value(&ev))
            />

            // category filter chips
            <div class="filter-row">
                <button
                    class=move || if cat_filter.get().is_empty() { "chip active" } else { "chip" }
                    on:click=move |_| cat_filter.set(String::new())
                >"All"</button>
                {move || {
                    domain::CATEGORIES.iter().map(|(id, emoji)| {
                        let cat = id.to_string();
                        let cat_cls = cat.clone();
                        view! {
                            <button
                                class=move || if cat_filter.get() == cat_cls { "chip active" } else { "chip" }
                                on:click=move |_| cat_filter.set(cat.clone())
                            >{*emoji}</button>
                        }
                    }).collect_view()
                }}
            </div>

            <Show when=move || products.get().is_empty()>
                <div class="hint">
                    "Your product database is empty — record a price from a store's page first."
                </div>
            </Show>

            <For
                each=move || filtered.get()
                key=|p| p.id
                children={move |p: Product| {
                    let pid = p.id;
                    let is_sel = move || selected.get().contains(&pid);
                    view! {
                        <div class="store-row" on:click=move |_| nav.set(Nav::ProductDetail(pid))>
                            <div class="s-info">
                                <div class="s-name">{domain::category_emoji(&p.category)}" "{p.name.clone()}</div>
                                <div class="s-sub">{move || summary_for(pid)}</div>
                            </div>
                            <button
                                class=move || if is_sel() { "row-check checked" } else { "row-check" }
                                title="Select to compare similar products"
                                on:click=move |ev| {
                                    ev.stop_propagation();
                                    toggle(pid);
                                }
                            >
                                {move || if is_sel() { "✓" } else { "" }}
                            </button>
                        </div>
                    }
                }}
            />

            <Show when=move || !selected.get().is_empty()>
                <button class="add-btn compare" on:click=compare_selected>
                    "⚖️ Compare selected (" {move || selected.get().len()} ")"
                </button>
            </Show>
        </div>
    }
}

// ---------------------------------------------------------------- comparison list

#[component]
fn ComparisonList(
    ids: Vec<u64>,
    stores: RwSignal<Vec<Store>>,
    products: RwSignal<Vec<Product>>,
    entries: RwSignal<Vec<PriceEntry>>,
    currencies: RwSignal<Vec<Currency>>,
    settings: RwSignal<Settings>,
    show_product: bool,
) -> impl IntoView {
    let ids_for_ranking = ids.clone();
    let ids_for_verdict = ids.clone();

    let ranking = Memo::new(move |_| {
        compute_ranking(
            &entries.get(),
            &stores.get(),
            &products.get(),
            &currencies.get(),
            &settings.get().main_currency,
            &ids_for_ranking,
        )
    });

    let verdict = Memo::new(move |_| {
        ranking
            .get()
            .into_iter()
            .find(|r| r.metric.is_some())
    });

    let max_metric = Memo::new(move |_| {
        ranking
            .get()
            .iter()
            .filter_map(|r| r.metric)
            .fold(f64::NAN, f64::max)
    });

    let main_sym = Memo::new(move |_| {
        symbol_of(&currencies.get(), &settings.get().main_currency)
    });

    let _ = ids_for_verdict;

    view! {
        // ---- verdict
        <Show when=move || verdict.get().is_some()>
            <div class="verdict">
                <div class="v-label">"BEST DEAL"</div>
                <div class="v-store">
                    {move || verdict.get().map(|r| r.store_name.clone()).unwrap_or_default()}
                </div>
                <div class="v-price">
                    {move || {
                        verdict.get().and_then(|r| r.metric).map(|m| {
                            format!("{}{}", main_sym.get(), fmt_price(m))
                        })
                    }}
                    <span class="v-unit">
                        {move || if verdict.get().map(|r| r.kg_mode).unwrap_or(false) { " /kg" } else { " each" }}
                    </span>
                </div>
                {move || {
                    let v = verdict.get();
                    let secondary = v.as_ref().and_then(|r| if r.kg_mode { r.per_unit } else { r.per_kg });
                    match secondary {
                        Some(u) => {
                            let unit = if v.map(|r| r.kg_mode).unwrap_or(false) { "each" } else { "/kg" };
                            Some(view! { <div class="v-sub">{main_sym.get()}{fmt_price(u)}" "{unit}</div> })
                        }
                        None => None,
                    }
                }}
                {move || {
                    let best = verdict.get().and_then(|r| r.metric);
                    let max = max_metric.get();
                    if let Some(best) = best {
                        if max.is_finite() {
                            let save = max - best;
                            if save > 1e-6 {
                                let unit = if verdict.get().map(|r| r.kg_mode).unwrap_or(false) { "/kg" } else { "each" };
                                return Some(view! {
                                    <div class="v-save">
                                        {"Save "}{main_sym.get()}{fmt_price(save)}{" "}{unit}{" vs the priciest store"}
                                    </div>
                                });
                            }
                        }
                    }
                    None
                }}
            </div>
        </Show>

        <Show when=move || {
            let rows = ranking.get();
            !rows.is_empty() && rows.iter().all(|r| r.metric.is_none())
        }>
            <div class="hint">
                "No comparable price yet — enter the weight of one unit (for /kg) or a plain unit price."
            </div>
        </Show>

        <For
            each=move || ranking.get()
            key=|r| r.entry_id
            children={move |r: RankRow| {
                let cls = if r.best { "rank-row best" } else { "rank-row" };
                let sid = r.store_id;
                view! {
                    <div class=cls>
                        <div class="rank-info">
                            <div class="rank-store">
                                {move || {
                                    let store = stores.get().iter().find(|s| s.id == sid).cloned();
                                    let name = r.store_name.clone();
                                    let query = store.as_ref().and_then(|s| s.address.clone())
                                        .filter(|a| !a.trim().is_empty())
                                        .unwrap_or_else(|| name.clone());
                                    let url = format!("https://maps.apple.com/?q={}", percent_encode(&query));
                                    let mut content = String::new();
                                    if show_product {
                                        content.push_str(&r.product_emoji.clone());
                                        content.push(' ');
                                        content.push_str(&r.product_name.clone());
                                        content.push_str(" · ");
                                    }
                                    content.push_str(&name);
                                    let url2 = url.clone();
                                    view! {
                                        <a class="maps-link" href=url2 target="_blank">{content}</a>
                                    }.into_any()
                                }}
                                <Show when=move || r.promo>
                                    <span class="promo-badge">"PROMO"</span>
                                </Show>
                            </div>
                            <div class="rank-date">
                                {if r.date.is_empty() { "—".to_string() } else { r.date.clone() }}
                            </div>
                        </div>
                        <div class="rank-prices">
                            {move || {
                                // Primary metric bold, secondary small.
                                if r.kg_mode {
                                    r.per_kg.map(|k| view! {
                                        <div class="rank-kg">{main_sym.get()}{fmt_price(k)}" /kg"</div>
                                        {r.per_unit.map(|u| view! { <div class="rank-unit">{main_sym.get()}{fmt_price(u)}" each"</div> })}
                                    }.into_any())
                                } else {
                                    r.per_unit.map(|u| view! {
                                        <div class="rank-kg">{main_sym.get()}{fmt_price(u)}" each"</div>
                                        {r.per_kg.map(|k| view! { <div class="rank-unit">{main_sym.get()}{fmt_price(k)}" /kg"</div> })}
                                    }.into_any())
                                }
                            }}
                            {move || {
                                let data = if r.best {
                                    Some(("rank-delta winner", "★ best".to_string()))
                                } else if r.metric.is_none() {
                                    Some(("rank-delta", "add weight or price".to_string()))
                                } else {
                                    r.delta
                                        .filter(|d| *d > 1e-6)
                                        .map(|d| ("rank-delta", format!("+{}{} vs best", main_sym.get(), fmt_price(d))))
                                };
                                data.map(|(cls, text)| view! { <div class=cls>{text}</div> })
                            }}
                            {move || {
                                r.orig_text.clone().map(|t| view! { <div class="rank-orig">{t}</div> })
                            }}
                        </div>
                    </div>
                }
            }}
        />

        <Show when=move || ranking.get().len() < 2>
            <div class="hint">"Record this product in another store to compare."</div>
        </Show>
    }
}

// ---------------------------------------------------------------- product detail

#[component]
fn ProductView(
    product_id: u64,
    stores: RwSignal<Vec<Store>>,
    products: RwSignal<Vec<Product>>,
    entries: RwSignal<Vec<PriceEntry>>,
    currencies: RwSignal<Vec<Currency>>,
    settings: RwSignal<Settings>,
    nav: RwSignal<Nav>,
) -> impl IntoView {
    let product_line = Memo::new(move |_| {
        products
            .get()
            .into_iter()
            .find(|p| p.id == product_id)
            .map(|p| format!("{} {}", domain::category_emoji(&p.category), p.name))
            .unwrap_or_else(|| "Product".to_string())
    });

    let delete_product = move |_| {
        products.update(|v| v.retain(|p| p.id != product_id));
        entries.update(|v| v.retain(|e| e.product_id != product_id));
        nav.set(Nav::Products);
    };

    view! {
        <div class="page">
            <button class="back-btn" on:click=move |_| nav.set(Nav::Products)>"← Products"</button>
            <h1 class="product-title">{move || product_line.get()}</h1>

            <ComparisonList
                ids=vec![product_id]
                stores
                products
                entries
                currencies
                settings
                show_product=false
            />

            <div class="hint">"Add more prices from a store's page — the comparison updates automatically."</div>

            <button class="clear-btn" on:click=delete_product>"Delete this product"</button>
        </div>
    }
}

// ---------------------------------------------------------------- compare view (selected products)

#[component]
fn CompareView(
    ids: Vec<u64>,
    stores: RwSignal<Vec<Store>>,
    products: RwSignal<Vec<Product>>,
    entries: RwSignal<Vec<PriceEntry>>,
    currencies: RwSignal<Vec<Currency>>,
    settings: RwSignal<Settings>,
    nav: RwSignal<Nav>,
) -> impl IntoView {
    let count = ids.len();
    view! {
        <div class="page">
            <button class="back-btn" on:click=move |_| nav.set(Nav::Products)>"← Products"</button>
            <h1 class="product-title">"⚖️ Comparison (" {count} "products)"</h1>

            <ComparisonList
                ids
                stores
                products
                entries
                currencies
                settings
                show_product=true
            />

            <div class="hint">"Tick similar products (e.g. green & yellow kiwi) in the Products tab to build a comparison."</div>
        </div>
    }
}

// ---------------------------------------------------------------- currency row

#[component]
fn CurrencyRow(
    code: String,
    currencies: RwSignal<Vec<Currency>>,
    settings: RwSignal<Settings>,
) -> impl IntoView {
    let code_rate = code.clone();
    // Handlers that live inside reactive children closures (<Show>/<For>) must
    // be Copy — they can only capture Copy values or fresh locals. So the
    // non-Copy currency code is stashed in a Copy RwSignal instead.
    let code_sig = RwSignal::new(code.clone());

    let cur = Memo::new(move |_| {
        currencies
            .get()
            .iter()
            .find(|c| c.code == code)
            .cloned()
    });

    // NOTE: no <Show> wrapper here — a reactive children closure would have to
    // rebuild the on:click/on:change handlers on every run, and those handlers
    // capture `code` by move (String is not Copy), which makes the children
    // closure FnOnce. The row only exists while the currency exists (For key),
    // so the guard is unnecessary anyway.
    view! {
        <div class="cur-row">
            <div class="cur-info">
                <div class="s-name">
                    {move || cur.get().map(|c| format!("{} {}", c.symbol, c.code)).unwrap_or_default()}
                </div>
                <div class="s-sub">
                    {move || {
                        cur.get().map(|c| {
                            format!("1 {} = {} {}", c.code, fmt_price(c.rate), settings.get().main_currency)
                        }).unwrap_or_default()
                    }}
                </div>
            </div>
            <input
                class="rate-input"
                inputmode="decimal"
                prop:value=move || cur.get().map(|c| c.rate.to_string()).unwrap_or_default()
                on:change=move |ev| {
                    if let Some(r) = domain::parse_number(&event_target_value(&ev)) {
                        currencies.update(|list| {
                            if let Some(c) = list.iter_mut().find(|c| c.code == code_rate) {
                                c.rate = r;
                            }
                        });
                    }
                }
            />
            <Show when=move || {
                cur.get().map(|c| c.code != settings.get().main_currency).unwrap_or(false)
            }>
                <button class="icon-btn small" title="Delete currency" on:click=move |_| {
                    let target = code_sig.get_untracked();
                    currencies.update(|v| v.retain(|c| c.code != target));
                }>"✕"</button>
            </Show>
        </div>
    }
}

// ---------------------------------------------------------------- settings view

#[component]
fn SettingsView(
    settings: RwSignal<Settings>,
    currencies: RwSignal<Vec<Currency>>,
) -> impl IntoView {
    let status = RwSignal::new(String::new());
    let adding = RwSignal::new(false);
    let new_code = RwSignal::new(String::new());
    let new_symbol = RwSignal::new(String::new());
    let new_rate = RwSignal::new(String::new());

    let main = move || settings.get().main_currency;

    let add_currency = move |_| {
        let code = new_code.get_untracked().trim().to_uppercase();
        let symbol = new_symbol.get_untracked().trim().to_string();
        let rate = domain::parse_number(&new_rate.get_untracked()).unwrap_or(1.0);
        if code.is_empty() {
            status.set("Enter a currency code (e.g. SGD)".to_string());
            return;
        }
        if currencies.get_untracked().iter().any(|c| c.code == code) {
            status.set(format!("{} already exists", code));
            return;
        }
        let symbol = if symbol.is_empty() {
            domain::currency_symbol(&code).to_string()
        } else {
            symbol
        };
        currencies.update(|v| v.push(Currency { code: code.clone(), symbol, rate }));
        new_code.set(String::new());
        new_symbol.set(String::new());
        new_rate.set(String::new());
        status.set(format!("{} added", code));
    };

    let fetch_live = move |_| {
        let base = main();
        status.set("Fetching live rates…".to_string());
        let currencies = currencies.clone();
        let status = status.clone();
        spawn_local(async move {
            let url = format!("https://api.frankfurter.dev/v1/latest?base={}", base);
            match gloo_net::http::Request::get(&url).send().await {
                Ok(resp) => match resp.json::<serde_json::Value>().await {
                    Ok(v) => {
                        let rates = v.get("rates").cloned().unwrap_or(serde_json::Value::Null);
                        let mut n = 0usize;
                        currencies.update(|list| {
                            for c in list.iter_mut() {
                                if c.code == base {
                                    c.rate = 1.0;
                                    continue;
                                }
                                if let Some(r) = rates.get(&c.code).and_then(|x| x.as_f64()) {
                                    if r > 0.0 {
                                        c.rate = 1.0 / r;
                                        n += 1;
                                    }
                                }
                            }
                        });
                        status.set(format!("Updated {} rates to {} base", n, base));
                    }
                    Err(_) => status.set("Could not parse rates".to_string()),
                },
                Err(_) => status.set("Failed — check your internet connection".to_string()),
            }
        });
    };

    view! {
        <div class="page">
            <header><h1>"⚙️ Settings"</h1></header>

            <h2 class="section">"Main currency"</h2>
            <div class="hint">
                "All comparisons are shown in your main currency. Record prices in any currency while travelling — they are converted automatically."
            </div>
            <select
                class="unit-select wide main-select"
                prop:value=move || main()
                on:change=move |ev| settings.update(|s| s.main_currency = event_target_value(&ev))
            >
                {move || {
                    currencies.get().iter().map(|c: &Currency| {
                        view! { <option value=c.code.clone()>{c.symbol.clone()}" "{c.code.clone()}</option> }
                    }).collect_view()
                }}
            </select>

            <h2 class="section">"Exchange rates"</h2>
            <p class="settings-note">"1 unit of currency = this many " {move || main()}</p>

            <For
                each=move || currencies.get()
                key=|c| c.code.clone()
                children={move |c: Currency| {
                    view! { <CurrencyRow code=c.code currencies settings /> }
                }}
            />

            <button class="add-btn" on:click=fetch_live>"↻ Fetch live rates"</button>
            <Show when=move || !status.get().is_empty()>
                <div class="hint">{move || status.get()}</div>
            </Show>

            <h2 class="section">"Add a currency"</h2>
            <Show
                when=move || adding.get()
                fallback=move || {
                    view! {
                        <button class="collapse-btn" on:click=move |_| adding.set(true)>
                            "＋ Add a currency"
                        </button>
                    }
                }
            >
                <div class="card">
                    <div class="grid3">
                        <label class="field">
                            <span class="lbl">"Code"</span>
                            <input
                                placeholder="SGD"
                                prop:value=move || new_code.get()
                                on:input=move |ev| new_code.set(event_target_value(&ev))
                            />
                        </label>
                        <label class="field">
                            <span class="lbl">"Symbol"</span>
                            <input
                                placeholder="S$"
                                prop:value=move || new_symbol.get()
                                on:input=move |ev| new_symbol.set(event_target_value(&ev))
                            />
                        </label>
                        <label class="field">
                            <span class="lbl">"Rate"</span>
                            <input
                                inputmode="decimal"
                                placeholder="0.68"
                                prop:value=move || new_rate.get()
                                on:input=move |ev| new_rate.set(event_target_value(&ev))
                            />
                        </label>
                    </div>
                    <button class="add-btn" on:click=add_currency>"Add currency"</button>
                </div>
            </Show>

            <footer>
                <p>"All data stays on this device (localStorage). Nothing is sent anywhere, except live-rate fetches and optional GPS address lookup."</p>
            </footer>
        </div>
    }
}

// ---------------------------------------------------------------- app

#[component]
fn App() -> impl IntoView {
    let stores: RwSignal<Vec<Store>> = RwSignal::new(storage::load_stores());
    let products: RwSignal<Vec<Product>> = RwSignal::new(storage::load_products());

    // Migrate old entries that have no currency → assume main currency.
    let main0 = storage::load_settings().main_currency;
    let mut loaded_entries = storage::load_entries();
    for e in loaded_entries.iter_mut() {
        if e.currency.is_empty() {
            e.currency = main0.clone();
        }
    }
    let entries: RwSignal<Vec<PriceEntry>> = RwSignal::new(loaded_entries);

    let currencies: RwSignal<Vec<Currency>> = RwSignal::new({
        let c = storage::load_currencies();
        if c.is_empty() {
            domain::default_currencies()
        } else {
            c
        }
    });
    let settings: RwSignal<Settings> = RwSignal::new(storage::load_settings());

    let nav: RwSignal<Nav> = RwSignal::new(Nav::Stores);

    // Persist on every change.
    Effect::new(move |_| storage::save_stores(&stores.get()));
    Effect::new(move |_| storage::save_products(&products.get()));
    Effect::new(move |_| storage::save_entries(&entries.get()));
    Effect::new(move |_| storage::save_currencies(&currencies.get()));
    Effect::new(move |_| storage::save_settings(&settings.get()));

    view! {
        <main>
            {move || match nav.get() {
                Nav::Stores => view! { <StoresView stores entries nav /> }.into_any(),
                Nav::StoreDetail(id) => {
                    view! {
                        <StoreView store_id=id stores products entries currencies settings nav />
                    }.into_any()
                }
                Nav::Products => {
                    view! {
                        <ProductsView products entries stores currencies settings nav />
                    }.into_any()
                }
                Nav::ProductDetail(id) => {
                    view! {
                        <ProductView product_id=id stores products entries currencies settings nav />
                    }.into_any()
                }
                Nav::Compare(ids) => {
                    view! {
                        <CompareView ids stores products entries currencies settings nav />
                    }.into_any()
                }
                Nav::Settings => view! { <SettingsView settings currencies /> }.into_any(),
            }}
            <TabBar nav />
        </main>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
