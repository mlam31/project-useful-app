//! Price Compare — a small database of stores → products → prices,
//! with fair per-kg / per-unit comparison across stores.
//! Black & white OLED-friendly PWA built with Leptos (CSR).

mod storage;

use leptos::prelude::*;
use leptos::task::spawn_local;
use useful_app::domain::{self, PriceEntry, Product, Store};
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

// ---------------------------------------------------------------- navigation

#[derive(Clone, Copy, PartialEq)]
enum Nav {
    Stores,
    StoreDetail(u64),
    Products,
    ProductDetail(u64),
}

// ---------------------------------------------------------------- ranking

#[derive(Clone, Debug, PartialEq)]
struct RankRow {
    entry_id: u64,
    store_id: u64,
    store_name: String,
    promo: bool,
    date: String,
    per_unit: Option<f64>,
    per_kg: Option<f64>,
    /// The value used for ranking: per-kg when any entry has a weight, else per-unit.
    metric: Option<f64>,
    delta: Option<f64>,
    best: bool,
    kg_mode: bool,
}

/// Rank the prices of ONE product across stores.
/// Only the most recent entry per store competes; stores without a
/// comparable metric go last.
fn compute_ranking(entries: &[PriceEntry], stores: &[Store], product_id: u64) -> Vec<RankRow> {
    let filtered: Vec<PriceEntry> = entries
        .iter()
        .filter(|e| e.product_id == product_id)
        .cloned()
        .collect();
    let latest = domain::latest_per_store(&filtered);

    let mut rows: Vec<RankRow> = latest
        .iter()
        .map(|e| {
            let q = domain::entry_quote(e);
            let store_name = stores
                .iter()
                .find(|s| s.id == e.store_id)
                .map(|s| s.name.trim().to_string())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("Store #{}", e.store_id));
            RankRow {
                entry_id: e.id,
                store_id: e.store_id,
                store_name,
                promo: e.promo,
                date: e.date.clone(),
                per_unit: q.per_unit,
                per_kg: q.per_kg,
                metric: None,
                delta: None,
                best: false,
                kg_mode: false,
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

/// Ask the browser for the current position and hand the resolved address
/// to `apply` (which writes it wherever needed).
fn pin_address(apply: impl Fn(String) + 'static, pinning: RwSignal<bool>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(geo) = window.navigator().geolocation().ok() else {
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

    let on_err = Closure::once(move |_e: web_sys::PositionError| {
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
    let stores_active = move || matches!(nav.get(), Nav::Stores | Nav::StoreDetail(_));
    let products_active = move || matches!(nav.get(), Nav::Products | Nav::ProductDetail(_));

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

            <div class="add-inline">
                <input
                    placeholder="New store name"
                    prop:value=move || new_name.get()
                    on:input=move |ev| new_name.set(event_target_value(&ev))
                />
                <button class="mini-add" on:click=add_store>"+ Add"</button>
            </div>

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

    // ---- add price form
    let new_product = RwSignal::new(String::new());
    let new_cat = RwSignal::new("other".to_string());
    let new_price = RwSignal::new(String::new());
    let new_qty = RwSignal::new("1".to_string());
    let new_weight = RwSignal::new(String::new());
    let new_wunit = RwSignal::new("g".to_string());
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
                let category = new_cat.get_untracked();
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
        entry.promo = new_promo.get_untracked();
        entry.date = new_date.get_untracked();
        entries.update(|v| v.push(entry));

        // Reset the form (keep the date, it usually stays "today").
        new_product.set(String::new());
        new_cat.set("other".to_string());
        new_price.set(String::new());
        new_qty.set("1".to_string());
        new_weight.set(String::new());
        new_promo.set(false);
    };

    // ---- store header
    let pinning = RwSignal::new(false);
    let pin = move |_| {
        pin_address(
            move |addr| update_store(stores, store_id, move |s| s.address = Some(addr)),
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
                    let price_txt = if qty != 1.0 {
                        format!("{} × {}", entry.price.trim(), qty)
                    } else {
                        entry.price.trim().to_string()
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

            // ---- add price card
            <h2 class="section">"Add a price"</h2>
            <div class="card">
                <input
                    class="store-input"
                    placeholder="Product (e.g. Apples, Bottle of water)"
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
                            prop:value=move || new_cat.get()
                            on:change=move |ev| new_cat.set(event_target_value(&ev))
                        >
                            {domain::CATEGORIES.iter().map(|(id, label)| {
                                view! { <option value=*id>{*label}</option> }
                            }).collect_view()}
                        </select>
                    </label>
                    <label class="field">
                        <span class="lbl">"Price (total)"</span>
                        <input
                            inputmode="decimal"
                            placeholder="4.99"
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
                            placeholder="1"
                            prop:value=move || new_qty.get()
                            on:input=move |ev| new_qty.set(event_target_value(&ev))
                        />
                    </label>
                    <label class="field">
                        <span class="lbl">"1 unit (weight)"</span>
                        <input
                            inputmode="decimal"
                            placeholder="200"
                            prop:value=move || new_weight.get()
                            on:input=move |ev| new_weight.set(event_target_value(&ev))
                        />
                    </label>
                    <label class="field">
                        <span class="lbl">"Unit"</span>
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

                <div class="row meta-row">
                    <label class="field date-field">
                        <span class="lbl">"Found on"</span>
                        <input
                            type="date"
                            prop:value=move || new_date.get()
                            on:input=move |ev| new_date.set(event_target_value(&ev))
                        />
                    </label>
                    <label class="check">
                        <input
                            type="checkbox"
                            prop:checked=move || new_promo.get()
                            on:change=move |ev| new_promo.set(event_target_checked(&ev))
                        />
                        <span>"Promo"</span>
                    </label>
                </div>

                <button class="add-btn" on:click=save_price>"Save price"</button>
            </div>

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
    nav: RwSignal<Nav>,
) -> impl IntoView {
    let search = RwSignal::new(String::new());

    let filtered = Memo::new(move |_| {
        let q = search.get().trim().to_lowercase();
        let mut list: Vec<Product> = products
            .get()
            .into_iter()
            .filter(|p| q.is_empty() || p.name.to_lowercase().contains(&q))
            .collect();
        list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        list
    });

    let summary_for = move |pid: u64| -> String {
        let rows = compute_ranking(&entries.get_untracked(), &stores.get_untracked(), pid);
        match rows.iter().find(|r| r.metric.is_some()) {
            Some(best) => {
                let m = best.metric.unwrap();
                let unit = if best.kg_mode { "/kg" } else { "each" };
                format!("from {} {} · {}", fmt_price(m), unit, best.store_name)
            }
            None => "no prices yet".to_string(),
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
                    view! {
                        <div class="store-row" on:click=move |_| nav.set(Nav::ProductDetail(pid))>
                            <div class="s-info">
                                <div class="s-name">{domain::category_emoji(&p.category)}" "{p.name.clone()}</div>
                                <div class="s-sub">{move || summary_for(pid)}</div>
                            </div>
                            <div class="chev">"›"</div>
                        </div>
                    }
                }}
            />
        </div>
    }
}

// ---------------------------------------------------------------- product detail (comparison)

#[component]
fn ProductView(
    product_id: u64,
    stores: RwSignal<Vec<Store>>,
    products: RwSignal<Vec<Product>>,
    entries: RwSignal<Vec<PriceEntry>>,
    nav: RwSignal<Nav>,
) -> impl IntoView {
    let ranking = Memo::new(move |_| compute_ranking(&entries.get(), &stores.get(), product_id));
    let verdict = Memo::new(move |_| ranking.get().into_iter().find(|r| r.metric.is_some()));
    let max_metric = Memo::new(move |_| {
        ranking
            .get()
            .iter()
            .filter_map(|r| r.metric)
            .fold(f64::NAN, f64::max)
    });

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

            <Show when=move || verdict.get().is_some()>
                <div class="verdict">
                    <div class="v-label">"BEST DEAL"</div>
                    <div class="v-store">
                        {move || verdict.get().map(|r| r.store_name.clone()).unwrap_or_default()}
                    </div>
                    <div class="v-price">
                        {move || {
                            verdict.get().and_then(|r| r.metric).map(fmt_price)
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
                                Some(view! { <div class="v-sub">{fmt_price(u)}" "{unit}</div> })
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
                                        <div class="v-save">{"Save "}{fmt_price(save)}{" "}{unit}{" vs the priciest store"}</div>
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

            <Show when=move || ranking.get().len() < 2>
                <div class="hint">"Record this product in another store to compare."</div>
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
                                        let name = r.store_name.clone();
                                        match stores.get().iter().find(|s| s.id == sid).and_then(|s| s.address.clone()).filter(|a| !a.trim().is_empty()) {
                                            Some(addr) => {
                                                let url = format!("https://maps.apple.com/?q={}", percent_encode(&addr));
                                                view! {
                                                    <a class="maps-link" href=url target="_blank">{name}</a>
                                                }.into_any()
                                            }
                                            None => {
                                                view! { <span on:click=move |_| nav.set(Nav::StoreDetail(sid))>{name}</span> }.into_any()
                                            }
                                        }
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
                                            <div class="rank-kg">{fmt_price(k)}" /kg"</div>
                                            {r.per_unit.map(|u| view! { <div class="rank-unit">{fmt_price(u)}" each"</div> })}
                                        }.into_any())
                                    } else {
                                        r.per_unit.map(|u| view! {
                                            <div class="rank-kg">{fmt_price(u)}" each"</div>
                                            {r.per_kg.map(|k| view! { <div class="rank-unit">{fmt_price(k)}" /kg"</div> })}
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
                                            .map(|d| ("rank-delta", format!("+{} vs best", fmt_price(d))))
                                    };
                                    data.map(|(cls, text)| view! { <div class=cls>{text}</div> })
                                }}
                            </div>
                        </div>
                    }
                }}
            />

            <div class="hint">"Add more prices from a store's page — the comparison updates automatically."</div>

            <button class="clear-btn" on:click=delete_product>"Delete this product"</button>
        </div>
    }
}

// ---------------------------------------------------------------- app

#[component]
fn App() -> impl IntoView {
    let stores: RwSignal<Vec<Store>> = RwSignal::new(storage::load_stores());
    let products: RwSignal<Vec<Product>> = RwSignal::new(storage::load_products());
    let entries: RwSignal<Vec<PriceEntry>> = RwSignal::new(storage::load_entries());
    let nav: RwSignal<Nav> = RwSignal::new(Nav::Stores);

    // Persist on every change.
    Effect::new(move |_| storage::save_stores(&stores.get()));
    Effect::new(move |_| storage::save_products(&products.get()));
    Effect::new(move |_| storage::save_entries(&entries.get()));

    view! {
        <main>
            {move || match nav.get() {
                Nav::Stores => view! { <StoresView stores entries nav /> }.into_any(),
                Nav::StoreDetail(id) => {
                    view! { <StoreView store_id=id stores products entries nav /> }.into_any()
                }
                Nav::Products => {
                    view! { <ProductsView products entries stores nav /> }.into_any()
                }
                Nav::ProductDetail(id) => {
                    view! { <ProductView product_id=id stores products entries nav /> }.into_any()
                }
            }}
            <TabBar nav />
        </main>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
