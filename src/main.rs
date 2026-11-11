//! Price Compare — a small database of stores → products → prices,
//! with fair per-kg / per-unit comparison across stores and multi-currency
//! support. Black & white OLED-friendly PWA built with Leptos (CSR).

mod storage;

use leptos::prelude::*;
use leptos::task::spawn_local;
use useful_app::domain::{self, Currency, PriceEntry, Product, Settings, Store};
use wasm_bindgen::prelude::*;
use web_sys::PositionOptions;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = window, js_name = startBarcodeScan)]
    fn js_start_barcode_scan(on_result: &js_sys::Function, on_error: &js_sys::Function) -> js_sys::Promise;
    #[wasm_bindgen(js_namespace = window, js_name = stopBarcodeScan)]
    fn js_stop_barcode_scan();
}

fn haptic() {
    if let Some(w) = web_sys::window() {
        let nav = w.navigator();
        let _ = js_sys::Reflect::get(&nav, &"vibrate".into())
            .ok()
            .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
            .map(|f| f.call1(&nav, &JsValue::from(10)));
    }
}

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

// ---------------------------------------------------------------- i18n

fn tr(lang: &str, key: &str) -> &'static str {
    match (lang, key) {
        ("fr", "stores") => "Magasins",
        ("es", "stores") => "Tiendas",
        ("zh", "stores") => "商店",
        (_, "stores") => "Stores",
        ("fr", "products") => "Produits",
        ("es", "products") => "Productos",
        ("zh", "products") => "产品",
        (_, "products") => "Products",
        ("fr", "scan") => "Scanner",
        ("es", "scan") => "Escanear",
        ("zh", "scan") => "扫描",
        (_, "scan") => "Scan",
        ("fr", "settings") => "Réglages",
        ("es", "settings") => "Ajustes",
        ("zh", "settings") => "设置",
        (_, "settings") => "Settings",
        ("fr", "language") => "Langue",
        ("es", "language") => "Idioma",
        ("zh", "language") => "语言",
        (_, "language") => "Language",
        ("fr", "app_language") => "Langue de l'app",
        ("es", "app_language") => "Idioma de la app",
        ("zh", "app_language") => "应用语言",
        (_, "app_language") => "App language",
        ("fr", "scan_title") => "Scanner & évaluer",
        ("es", "scan_title") => "Escanear y evaluar",
        ("zh", "scan_title") => "扫描与评分",
        (_, "scan_title") => "Scan & rate",
        _ => "",
    }
}

// ---------------------------------------------------------------- navigation

#[derive(Clone, PartialEq)]
enum Nav {
    Stores,
    StoreDetail(u64),
    Products,
    ProductDetail(u64),
    Compare(Vec<u64>),
    Scan,
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
    per_l: Option<f64>,
    /// The value used for ranking: per-kg when any entry has a weight,
    /// else per-liter when any entry was bought by volume, else per-unit.
    metric: Option<f64>,
    delta: Option<f64>,
    best: bool,
    kg_mode: bool,
    l_mode: bool,
    /// Original values in the recorded currency (when different from main).
    orig_text: Option<String>,
    /// The same prices converted into the second display currency.
    second_text: Option<String>,
}

/// Label for the ranking basis.
fn metric_label(kg_mode: bool, l_mode: bool) -> &'static str {
    if kg_mode {
        "/kg"
    } else if l_mode {
        "/L"
    } else {
        "each"
    }
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
    second_code: &str,
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
            // Treat main currency as 1:1 to avoid stale-rate issues.
            let rate = if cur == main_code { 1.0 } else { currency_rate(currencies, &cur) };

            let orig_text = if cur != main_code {
                let sym = symbol_of(currencies, &cur);
                let mut parts = Vec::new();
                if let Some(k) = q.per_kg {
                    parts.push(format!("{}{} /kg", sym, fmt_price(k)));
                }
                if let Some(v) = q.per_l {
                    parts.push(format!("{}{} /L", sym, fmt_price(v)));
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

            // Same prices converted into the optional second display currency
            // (skipped when the entry already IS in the second currency —
            // the original values are shown above instead).
            let rate2 = if second_code.is_empty() {
                0.0
            } else {
                currency_rate(currencies, second_code)
            };
            let second_text = if !second_code.is_empty()
                && second_code != main_code
                && cur != second_code
                && rate2 > 0.0
            {
                let sym2 = symbol_of(currencies, second_code);
                let to_second = rate / rate2;
                let mut parts = Vec::new();
                if let Some(k) = q.per_kg {
                    parts.push(format!("{}{} /kg", sym2, fmt_price(k * to_second)));
                }
                if let Some(v) = q.per_l {
                    parts.push(format!("{}{} /L", sym2, fmt_price(v * to_second)));
                }
                if let Some(u) = q.per_unit {
                    parts.push(format!("{}{} each", sym2, fmt_price(u * to_second)));
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
                per_l: q.per_l.map(|v| v * rate),
                metric: None,
                delta: None,
                best: false,
                kg_mode: false,
                l_mode: false,
                orig_text,
                second_text,
            }
        })
        .collect();

    let kg_mode = rows.iter().any(|r| r.per_kg.is_some());
    let l_mode = !kg_mode && rows.iter().any(|r| r.per_l.is_some());
    for r in rows.iter_mut() {
        r.kg_mode = kg_mode;
        r.l_mode = l_mode;
        r.metric = if kg_mode {
            r.per_kg
        } else if l_mode {
            r.per_l
        } else {
            r.per_unit
        };
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
// Currently disabled: Safari only grants geolocation on HTTPS, which doesn't
// apply while testing over plain HTTP on the LAN. Kept here so the 📍 button
// can be re-enabled once the app is deployed over HTTPS.

#[allow(dead_code)]
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
#[allow(dead_code)]
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
fn TabBar(nav: RwSignal<Nav>, settings: RwSignal<Settings>) -> impl IntoView {
    let stores_active = move || {
        matches!(nav.get(), Nav::Stores | Nav::StoreDetail(_))
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
                {move || tr(&settings.get().language, "stores").to_string()}
            </button>
            <button
                class=move || if products_active() { "tab active" } else { "tab" }
                on:click=move |_| nav.set(Nav::Products)
            >
                {move || tr(&settings.get().language, "products").to_string()}
            </button>
            <button
                class=move || if settings_active() { "tab active" } else { "tab" }
                on:click=move |_| nav.set(Nav::Settings)
            >
                {move || tr(&settings.get().language, "settings").to_string()}
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
        haptic();
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
            <header><h1>"Stores"</h1></header>

            <div class="search-wrap">
                <input
                    class="search-input"
                    style="margin-bottom:0; padding-right:44px;"
                    placeholder="Search stores"
                    prop:value=move || search.get()
                    on:input=move |ev| search.set(event_target_value(&ev))
                />
                <button class="search-add" title="Add store" on:click=move |_| { haptic(); adding.update(|v| *v = !*v); }>"+"</button>
            </div>

            <Show when=move || adding.get()>
                <div class="card" style="margin-top:10px;">
                    <div class="row">
                        <input
                            class="store-input"
                            placeholder="New store name"
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

    // Tick entries in this store to compare or delete them.
    let selected_entries: RwSignal<Vec<u64>> = RwSignal::new(Vec::new());
    let toggle_entry = move |eid: u64| {
        selected_entries.update(|v| {
            if let Some(i) = v.iter().position(|&x| x == eid) { v.remove(i); } else { v.push(eid); }
        });
    };
    let compare_selected = move |_| {
        let sel = selected_entries.get_untracked();
        let mut ids: Vec<u64> = entries
            .get_untracked()
            .iter()
            .filter(|e| sel.contains(&e.id))
            .map(|e| e.product_id)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        if !ids.is_empty() {
            selected_entries.set(Vec::new());
            nav.set(Nav::Compare(ids));
        }
    };
    let delete_selected = move |_| {
        let sel = selected_entries.get_untracked();
        if sel.is_empty() {
            return;
        }
        for eid in &sel {
            let mut removed_pid: Option<u64> = None;
            entries.update(|v| {
                if let Some(pos) = v.iter().position(|e| e.id == *eid) {
                    removed_pid = Some(v[pos].product_id);
                    v.remove(pos);
                }
            });
            if let Some(pid) = removed_pid {
                let still_used = entries.get_untracked().iter().any(|e| e.product_id == pid);
                if !still_used {
                    products.update(|v| v.retain(|p| p.id != pid));
                }
            }
        }
        selected_entries.set(Vec::new());
    };

    // ---- add/edit price form (collapsed by default)
    let form_open = RwSignal::new(false);
    let editing_id: RwSignal<Option<u64>> = RwSignal::new(None);
    let menu_entry: RwSignal<Option<u64>> = RwSignal::new(None);
    let copy_entry_id: RwSignal<Option<u64>> = RwSignal::new(None);
    let new_product = RwSignal::new(String::new());
    let new_cat = RwSignal::new("other".to_string());
    let cat_touched = RwSignal::new(false);
    let new_price = RwSignal::new(String::new());
    let new_qty = RwSignal::new("1".to_string());
    let new_qtyunit = RwSignal::new("pc".to_string());
    let new_weight = RwSignal::new(String::new());
    let new_wunit = RwSignal::new("g".to_string());
    let new_cur = RwSignal::new(settings.get_untracked().main_currency.clone());
    let new_promo = RwSignal::new(false);
    let new_deal = RwSignal::new(String::new());
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

    let reset_form = move || {
        new_product.set(String::new());
        new_cat.set("other".to_string());
        cat_touched.set(false);
        new_price.set(String::new());
        if new_qtyunit.get_untracked() == "pc" {
            new_qty.set("1".to_string());
        } else {
            new_qty.set(String::new());
        }
        new_weight.set(String::new());
        new_promo.set(false);
        new_deal.set(String::new());
        editing_id.set(None);
        form_open.set(false);
    };

    let save_price = move |_| {
        haptic();
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
                products.update(|v| v.push(Product {
                    id,
                    name: name.clone(),
                    category,
                    barcode: None,
                    grade: None,
                    bad_ingredients: Vec::new(),
                    good_ingredients: Vec::new(),
                }));
                id
            });

        let qty_val = new_qty.get_untracked();
        let qty_val = if qty_val.trim().is_empty() { "1".to_string() } else { qty_val };
        let cur_val = if new_cur.get_untracked().is_empty() {
            settings.get_untracked().main_currency.clone()
        } else {
            new_cur.get_untracked()
        };

        if let Some(edit_eid) = editing_id.get_untracked() {
            entries.update(|v| {
                if let Some(e) = v.iter_mut().find(|e| e.id == edit_eid) {
                    e.product_id = pid;
                    e.price = new_price.get_untracked();
                    e.quantity = qty_val.clone();
                    e.qty_unit = new_qtyunit.get_untracked();
                    e.unit_weight = new_weight.get_untracked();
                    e.weight_unit = new_wunit.get_untracked();
                    e.currency = cur_val.clone();
                    e.promo = new_promo.get_untracked();
                    e.promo_deal = new_deal.get_untracked();
                    e.date = new_date.get_untracked();
                }
            });
        } else {
            let eid = next_id(entries.get_untracked().iter().map(|e| e.id));
            let mut entry = PriceEntry::new(eid, store_id, pid);
            entry.price = new_price.get_untracked();
            entry.quantity = qty_val;
            entry.qty_unit = new_qtyunit.get_untracked();
            entry.unit_weight = new_weight.get_untracked();
            entry.weight_unit = new_wunit.get_untracked();
            entry.currency = cur_val;
            entry.promo = new_promo.get_untracked();
            entry.promo_deal = new_deal.get_untracked();
            entry.date = new_date.get_untracked();
            entries.update(|v| v.push(entry));
        }

        reset_form();
    };

    // ---- store header
    // 📍 GPS pin is disabled for now: Safari only grants geolocation on HTTPS,
    // which doesn't apply while testing over plain HTTP on the LAN. The helper
    // (pin_address) stays in the code — re-enable the button below once the
    // app is deployed over HTTPS.

    let delete_store = move |_| {
        stores.update(|v| v.retain(|s| s.id != store_id));
        entries.update(|v| v.retain(|e| e.store_id != store_id));
        nav.set(Nav::Stores);
    };

    // ⋯ menu keeps destructive actions out of the way.
    let menu_open = RwSignal::new(false);
    let toggle_menu = move |_| menu_open.set(!menu_open.get_untracked());

    view! {
        <div class="page">
            <div class="page-head">
                <button class="back-btn" on:click=move |_| nav.set(Nav::Stores)>"← Stores"</button>
                <button class="dots-btn" title="More options" on:click=toggle_menu>"⋯"</button>
            </div>
            <Show when=move || menu_open.get()>
                <div class="menu-card">
                    <button class="menu-item" on:click=move |_| {
                        menu_open.set(false);
                        delete_store(());
                    }>"Delete this store"</button>
                </div>
            </Show>

            <input
                class="store-title"
                placeholder="Store name"
                prop:value=move || stores.get().iter().find(|s| s.id == store_id).map(|s| s.name.clone()).unwrap_or_default()
                on:input=move |ev| update_store(stores, store_id, move |s| s.name = event_target_value(&ev))
            />

            <input
                class="addr-input"
                placeholder="Address (optional)"
                prop:value=move || stores.get().iter().find(|s| s.id == store_id).and_then(|s| s.address.clone()).unwrap_or_default()
                on:input=move |ev| update_store(stores, store_id, move |s| s.address = Some(event_target_value(&ev)))
            />

            // ---- add / edit product card
            <h2 class="section">{move || if editing_id.get().is_some() { "Edit price" } else { "Add a product" }}</h2>
            <Show
                when=move || form_open.get()
                fallback=move || {
                    view! {
                        <button class="collapse-btn" on:click=move |_| { editing_id.set(None); form_open.set(true); }>
                            "Add a product"
                        </button>
                    }
                }
            >
                <div class="card">
                    <div class="row form-head">
                        <span class="form-title">{move || if editing_id.get().is_some() { "Edit price" } else { "New product & price" }}</span>
                        <button class="icon-btn small" title="Cancel" on:click=move |_| reset_form()>"✕"</button>
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
                                    domain::CATEGORIES.iter().map(|(id, emoji, label)| {
                                        view! { <option value=*id>{format!("{} {}", emoji, label)}</option> }
                                    }).collect_view()
                                }}
                            </select>
                        </label>
                        <label class="field">
                            <span class="lbl">"Price for this quantity"</span>
                            <input
                                inputmode="decimal"
                                placeholder="7.64"
                                prop:value=move || new_price.get()
                                on:input=move |ev| new_price.set(event_target_value(&ev))
                            />
                        </label>
                    </div>

                    <div class="grid2">
                        <label class="field">
                            <span class="lbl">"Quantity (how many / how much)"</span>
                            <div class="stepper">
                                <button class="step-btn" type="button" on:click=move |_| {
                                    let v = domain::parse_number(&new_qty.get_untracked()).unwrap_or(1.0);
                                    let nv = if v <= 1.0 { 1.0 } else { (v - 1.0).round() };
                                    new_qty.set(if (nv - nv.round()).abs() < 1e-9 { format!("{}", nv as i64) } else { fmt_price(nv) });
                                }>"−"</button>
                                <input
                                    inputmode="decimal"
                                    placeholder=move || if new_qtyunit.get() == "pc" { "1" } else { "500" }
                                    prop:value=move || new_qty.get()
                                    on:input=move |ev| new_qty.set(event_target_value(&ev))
                                />
                                <button class="step-btn" type="button" on:click=move |_| {
                                    let v = domain::parse_number(&new_qty.get_untracked()).unwrap_or(0.0);
                                    let nv = (v + 1.0).max(1.0);
                                    new_qty.set(if (nv - nv.round()).abs() < 1e-9 { format!("{}", nv as i64) } else { fmt_price(nv) });
                                }>"+"</button>
                            </div>
                        </label>
                        <label class="field">
                            <span class="lbl">"Unit"</span>
                            <select
                                class="unit-select wide"
                                prop:value=move || new_qtyunit.get()
                                on:change=move |ev| new_qtyunit.set(event_target_value(&ev))
                            >
                                <option value="pc">"items/pieces"</option>
                                <option value="g">"grams (total)"</option>
                                <option value="kg">"kg (total)"</option>
                                <option value="l">"liters (total)"</option>
                                <option value="ml">"mL (total)"</option>
                            </select>
                        </label>
                    </div>
                    <Show when=move || new_qtyunit.get() == "pc">
                        <div class="hint" style="margin-top:6px;">"For items sold by piece: set quantity = number of items and weight per item (e.g. 2 pots × 500g). Or switch unit to grams and enter the total (e.g. 1000g)."</div>
                    </Show>

                    <Show when=move || new_qtyunit.get() == "pc">
                        <div class="grid2">
                            <label class="field">
                                <span class="lbl">"Weight per item"</span>
                                <input
                                    inputmode="decimal"
                                    placeholder="500"
                                    prop:value=move || new_weight.get()
                                    on:input=move |ev| new_weight.set(event_target_value(&ev))
                                />
                            </label>
                            <label class="field">
                                <span class="lbl">"Weight unit"</span>
                                <select
                                    class="unit-select wide"
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
                    </Show>

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
                    <Show when=move || new_promo.get()>
                        <label class="field">
                            <span class="lbl">"Deal (optional)"</span>
                            <input
                                class="store-input"
                                placeholder="e.g. 50% off, 2 for $5"
                                prop:value=move || new_deal.get()
                                on:input=move |ev| new_deal.set(event_target_value(&ev))
                            />
                        </label>
                    </Show>

                    <button class="add-btn" on:click=save_price>{move || if editing_id.get().is_some() { "Save changes" } else { "Save product & price" }}</button>
                </div>
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
                <div class="hint">"No prices recorded yet. Add the first one above."</div>
            </Show>

            <For
                each=move || rows.get()
                key=|row| row.0.id
                children={move |row: (PriceEntry, String, String)| {
                    let entry = row.0;
                    let eid = entry.id;
                    let quote = domain::entry_quote(&entry);
                    let emoji = domain::category_emoji(&row.2);
                    let cur = if entry.currency.is_empty() {
                        settings.get_untracked().main_currency.clone()
                    } else {
                        entry.currency.clone()
                    };
                    let main_code = settings.get_untracked().main_currency;
                    let second = settings.get_untracked().second_currency.clone().unwrap_or_default();
                    let cur_list = currencies.get_untracked();

                    let rate_to_main = if cur == main_code { 1.0 } else { currency_rate(&cur_list, &cur) };

                    let main_sym = symbol_of(&cur_list, &main_code);
                    let entry_sym = symbol_of(&cur_list, &cur);

                    // Exact quantity label, e.g. "500g", "1.5L", "3 pc"
                    let qty_num = domain::parse_number(&entry.quantity).unwrap_or(1.0);
                    let qty_label = match entry.qty_unit.as_str() {
                        "g" => format!("{}g", entry.quantity.trim()),
                        "kg" => format!("{}kg", entry.quantity.trim()),
                        "l" => format!("{}L", entry.quantity.trim()),
                        "ml" => format!("{}mL", entry.quantity.trim()),
                        _ => {
                            if (qty_num - 1.0).abs() < 1e-9 {
                                "1 pc".to_string()
                            } else {
                                format!("{} pc", entry.quantity.trim())
                            }
                        }
                    };
                    // Card: exact quantity and price, e.g. "500g · $7.64"
                    let paid_txt = format!("{} · {}{}", qty_label, entry_sym, entry.price.trim());

                    // Details (simplified): date \ price currency1 per kg|L|each \ price currency2 per kg|L|each
                    let mut details: Vec<String> = Vec::new();
                    if !entry.date.trim().is_empty() {
                        details.push(entry.date.trim().to_string());
                    }
                    if let Some(k) = quote.per_kg {
                        details.push(format!("{}{} /kg", main_sym, fmt_price(k * rate_to_main)));
                        if !second.is_empty() && second != main_code {
                            let r2 = currency_rate(&cur_list, &second);
                            if r2 > 0.0 {
                                details.push(format!("{}{} /kg", symbol_of(&cur_list, &second), fmt_price(k * rate_to_main / r2)));
                            }
                        }
                    } else if let Some(v) = quote.per_l {
                        details.push(format!("{}{} /L", main_sym, fmt_price(v * rate_to_main)));
                        if !second.is_empty() && second != main_code {
                            let r2 = currency_rate(&cur_list, &second);
                            if r2 > 0.0 {
                                details.push(format!("{}{} /L", symbol_of(&cur_list, &second), fmt_price(v * rate_to_main / r2)));
                            }
                        }
                    } else if let Some(u) = quote.per_unit {
                        details.push(format!("{}{} each", main_sym, fmt_price(u * rate_to_main)));
                        if !second.is_empty() && second != main_code {
                            let r2 = currency_rate(&cur_list, &second);
                            if r2 > 0.0 {
                                details.push(format!("{}{} each", symbol_of(&cur_list, &second), fmt_price(u * rate_to_main / r2)));
                            }
                        }
                    }
                    let (detail_date, detail_prices) = if !entry.date.trim().is_empty() {
                        (entry.date.trim().to_string(), if details.len() > 1 { details[1..].join(" · ") } else { String::new() })
                    } else {
                        (String::new(), details.join(" · "))
                    };

                    // History: other/previous prices for same product in this store
                    let mut history: Vec<String> = Vec::new();
                    let mut hist_entries: Vec<PriceEntry> = entries.get_untracked().into_iter()
                        .filter(|e| e.store_id == entry.store_id && e.product_id == entry.product_id)
                        .collect();
                    hist_entries.sort_by(|a, b| b.date.cmp(&a.date).then(b.id.cmp(&a.id)));
                    for h in hist_entries.iter().skip(1).take(5) {
                        let hs = symbol_of(&cur_list, if h.currency.is_empty() { &main_code } else { &h.currency });
                        let hqty = match h.qty_unit.as_str() {
                            "g" => format!("{}g", h.quantity.trim()),
                            "kg" => format!("{}kg", h.quantity.trim()),
                            "l" => format!("{}L", h.quantity.trim()),
                            "ml" => format!("{}mL", h.quantity.trim()),
                            _ => format!("{} pc", h.quantity.trim()),
                        };
                        history.push(format!("{} — {} · {}{}", h.date, hqty, hs, h.price.trim()));
                    }
                    let history_txt = history.join("\n");

                    let badge_txt = if entry.promo {
                        let d = entry.promo_deal.trim();
                        if d.is_empty() {
                            "PROMO".to_string()
                        } else if let Some(pct) = d.split_whitespace().find(|t| t.contains('%')) {
                            let p = pct.trim_matches(|c: char| !c.is_ascii_digit() && c != '%' && c != '.');
                            if p.contains('%') {
                                let num = p.trim_end_matches('%').trim_start_matches('-');
                                if num.is_empty() { "-%".to_string() } else { format!("-{}%", num) }
                            } else {
                                d.to_uppercase()
                            }
                        } else {
                            d.to_uppercase()
                        }
                    } else {
                        String::new()
                    };

                    let entry_sv = StoredValue::new(entry.clone());
                    let detail_date_sv = StoredValue::new(detail_date.clone());
                    let detail_prices_sv = StoredValue::new(detail_prices.clone());
                    let history_sv = StoredValue::new(history_txt.clone());
                    let badge_sv = StoredValue::new(badge_txt.clone());
                    let paid_sv = StoredValue::new(paid_txt.clone());
                    let name_sv = StoredValue::new(row.1.clone());
                    let is_sel = move || selected_entries.get().contains(&eid);
                    let is_menu = move || menu_entry.get() == Some(eid);
                    let is_copying = move || copy_entry_id.get() == Some(eid);

                    view! {
                        <div class="entry-card">
                            <div class="entry-row" on:click=move |_| {
                                menu_entry.update(|m| *m = if *m == Some(eid) { None } else { Some(eid) });
                                copy_entry_id.set(None);
                            }>
                                <button
                                    class=move || if is_sel() { "row-check checked" } else { "row-check" }
                                    title="Select"
                                    on:click=move |ev| { ev.stop_propagation(); toggle_entry(eid); }
                                >{move || if is_sel() { "✓" } else { "" }}</button>
                                <div class="e-info">
                                    <div class="e-name">{emoji}" "{move || name_sv.get_value()}</div>
                                </div>
                                <div class="e-right">
                                    <div class="e-paid">{move || paid_sv.get_value()}</div>
                                    {move || {
                                        let e = entry_sv.get_value();
                                        if e.promo { Some(view! { <span class="promo-badge">{badge_sv.get_value()}</span> }) } else { None }
                                    }}
                                    {move || {
                                        let pid = entry_sv.get_value().product_id;
                                        products.get().iter().find(|p| p.id == pid).and_then(|p| p.grade).map(|g| {
                                            let cls = if g >= 75 { "rate-badge good" } else if g >= 40 { "rate-badge mid" } else { "rate-badge bad" };
                                            view! { <span class=cls>{format!("{}/100", g)}</span> }
                                        })
                                    }}
                                </div>
                            </div>

                            <Show when=is_menu>
                                <div class="entry-menu">
                                    <div class="entry-detail">
                                        <span class="ed-date">{move || detail_date_sv.get_value()}</span>
                                        <span class="ed-prices">{move || detail_prices_sv.get_value()}</span>
                                    </div>
                                    {move || {
                                        let h = history_sv.get_value();
                                        if h.is_empty() { None } else {
                                            Some(view! { <div class="entry-history"><div class="eh-title">"Price history"</div><pre class="eh-body">{h}</pre></div> })
                                        }
                                    }}
                                    <div class="entry-actions">
                                        <button class="menu-item" on:click=move |_| {
                                            let e = entry_sv.get_value();
                                            let pname = products.get_untracked().iter().find(|p| p.id == e.product_id).map(|p| p.name.clone()).unwrap_or_default();
                                            let pcat = products.get_untracked().iter().find(|p| p.id == e.product_id).map(|p| p.category.clone()).unwrap_or_else(|| "other".to_string());
                                            new_product.set(pname);
                                            new_cat.set(pcat);
                                            cat_touched.set(true);
                                            new_price.set(e.price.clone());
                                            new_qty.set(e.quantity.clone());
                                            new_qtyunit.set(e.qty_unit.clone());
                                            new_weight.set(e.unit_weight.clone());
                                            new_wunit.set(e.weight_unit.clone());
                                            new_cur.set(if e.currency.is_empty() { settings.get_untracked().main_currency } else { e.currency.clone() });
                                            new_promo.set(e.promo);
                                            new_deal.set(e.promo_deal.clone());
                                            new_date.set(today_iso());
                                            editing_id.set(None); // save as new entry to keep history
                                            form_open.set(true);
                                            menu_entry.set(None);
                                        }>"New price"</button>
                                        <button class="menu-item" on:click=move |_| {
                                            copy_entry_id.update(|c| *c = if *c == Some(eid) { None } else { Some(eid) });
                                        }>"Copy to another store"</button>
                                    </div>
                                    <Show when=is_copying>
                                        <div class="copy-list">
                                            <div class="hint" style="margin:6px 0;">"Pick a store:"</div>
                                            {move || {
                                                let src = entry_sv.get_value();
                                                stores.get().into_iter().filter(|s| s.id != src.store_id).map(|s| {
                                                    let src2 = src.clone();
                                                    let sid = s.id;
                                                    let sname = s.name.clone();
                                                    view! {
                                                        <button class="chip" style="margin:2px;" on:click=move |_| {
                                                            let mut ne = src2.clone();
                                                            let nid = next_id(entries.get_untracked().iter().map(|e| e.id));
                                                            ne.id = nid;
                                                            ne.store_id = sid;
                                                            ne.date = today_iso();
                                                            entries.update(|v| v.push(ne));
                                                            copy_entry_id.set(None);
                                                            menu_entry.set(None);
                                                        }>{sname}</button>
                                                    }
                                                }).collect_view()
                                            }}
                                        </div>
                                    </Show>
                                </div>
                            </Show>
                        </div>
                    }
                }}
            />

            <Show when=move || !selected_entries.get().is_empty()>
                <div class="row" style="gap:8px;">
                    <button class="add-btn compare" style="flex:1; margin-top:8px;" on:click=move |_| { haptic(); compare_selected(()); }>
                        "Compare (" {move || selected_entries.get().len()} ")"
                    </button>
                    <button class="add-btn danger" style="flex:1; margin-top:8px; background:#1a0a0a; color:#fff; border-color:#844;" on:click=move |_| { haptic(); delete_selected(()); }>
                        "Delete (" {move || selected_entries.get().len()} ")"
                    </button>
                </div>
            </Show>

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
        let second = settings
            .get_untracked()
            .second_currency
            .clone()
            .unwrap_or_default();
        let rows = compute_ranking(
            &entries.get_untracked(),
            &stores.get_untracked(),
            &products.get_untracked(),
            &currencies.get_untracked(),
            &settings.get_untracked().main_currency,
            &second,
            &[pid],
        );
        match rows.iter().find(|r| r.metric.is_some()) {
            Some(best) => {
                let m = best.metric.unwrap();
                let sym = symbol_of(&currencies.get_untracked(), &settings.get_untracked().main_currency);
                let unit = metric_label(best.kg_mode, best.l_mode);
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
            <header><h1>"Products"</h1></header>

            <input
                class="search-input"
                placeholder="Search products (fruits, vegetables, drinks, frozen, processed…)"
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
                    domain::CATEGORIES.iter().map(|(id, emoji, label)| {
                        let cat = id.to_string();
                        let cat_cls = cat.clone();
                        view! {
                            <button
                                class=move || if cat_filter.get() == cat_cls { "chip active" } else { "chip" }
                                on:click=move |_| cat_filter.set(cat.clone())
                            >{*emoji}" "{*label}</button>
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
                <button class="add-btn compare" on:click=move |_| { haptic(); compare_selected(()); }>
                    "Compare selected (" {move || selected.get().len()} ")"
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
        let second = settings
            .get()
            .second_currency
            .clone()
            .unwrap_or_default();
        compute_ranking(
            &entries.get(),
            &stores.get(),
            &products.get(),
            &currencies.get(),
            &settings.get().main_currency,
            &second,
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
                        {move || {
                            verdict.get().map(|r| format!(" {}", metric_label(r.kg_mode, r.l_mode)))
                                .unwrap_or_default()
                        }}
                    </span>
                </div>
                {move || {
                    let v = verdict.get();
                    let secondary = v.as_ref().and_then(|r| {
                        if r.kg_mode {
                            r.per_unit
                        } else if r.l_mode {
                            None
                        } else {
                            r.per_kg
                        }
                    });
                    match secondary {
                        Some(u) => {
                            let kg = v.as_ref().map(|r| r.kg_mode).unwrap_or(false);
                            let unit = if kg { "each" } else { "/kg" };
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
                                let v = verdict.get();
                                let unit = v.map(|r| metric_label(r.kg_mode, r.l_mode)).unwrap_or("each");
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
                                } else if r.l_mode {
                                    r.per_l.map(|v| view! {
                                        <div class="rank-kg">{main_sym.get()}{fmt_price(v)}" /L"</div>
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
                                    Some(("rank-delta winner", "★".to_string()))
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
                            {move || {
                                r.second_text.clone().map(|t| view! { <div class="rank-sec">{t}</div> })
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

    let menu_open = RwSignal::new(false);
    let toggle_menu = move |_| menu_open.set(!menu_open.get_untracked());

    view! {
        <div class="page">
            <div class="page-head">
                <button class="back-btn" on:click=move |_| nav.set(Nav::Products)>"← Products"</button>
                <button class="dots-btn" title="More options" on:click=toggle_menu>"⋯"</button>
            </div>
            <Show when=move || menu_open.get()>
                <div class="menu-card">
                    <button class="menu-item" on:click=move |_| {
                        menu_open.set(false);
                        delete_product(());
                    }>"Delete this product"</button>
                </div>
            </Show>
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
            <h1 class="product-title">"Comparison (" {count} " products)"</h1>

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

// ---------------------------------------------------------------- scan view

#[derive(Clone, Debug, Default, PartialEq)]
struct ScanResult {
    barcode: String,
    name: String,
    brand: String,
    quantity: String,
    grade: u8,
    nutriscore: String,
    nova: String,
    good: Vec<String>,
    bad: Vec<String>,
}

fn compute_grade(nutriscore: &str, nova: Option<i64>, additives_n: usize, eco: &str) -> (u8, Vec<String>, Vec<String>) {
    let mut score: i32 = match nutriscore.to_lowercase().as_str() {
        "a" => 90,
        "b" => 75,
        "c" => 55,
        "d" => 35,
        "e" => 15,
        _ => 50,
    };
    let mut good = Vec::new();
    let mut bad = Vec::new();

    match nutriscore.to_lowercase().as_str() {
        "a" => good.push("Nutri-Score A".to_string()),
        "b" => good.push("Nutri-Score B".to_string()),
        "d" => bad.push("Nutri-Score D".to_string()),
        "e" => bad.push("Nutri-Score E".to_string()),
        _ => {}
    }
    if let Some(n) = nova {
        match n {
            1 => { score += 5; good.push("NOVA 1 (unprocessed)".to_string()); }
            2 => { good.push("NOVA 2 (processed ingredients)".to_string()); }
            3 => { score -= 5; bad.push("NOVA 3 (processed)".to_string()); }
            4 => { score -= 12; bad.push("NOVA 4 (ultra-processed)".to_string()); }
            _ => {}
        }
    }
    if additives_n == 0 {
        score += 3;
        good.push("No additives reported".to_string());
    } else if additives_n <= 2 {
        score -= 4;
        bad.push(format!("{} additive(s)", additives_n));
    } else {
        score -= 10;
        bad.push(format!("{} additives", additives_n));
    }
    match eco.to_lowercase().as_str() {
        "a" => { score += 3; good.push("Eco-Score A".to_string()); }
        "b" => { score += 1; good.push("Eco-Score B".to_string()); }
        "e" => { score -= 3; bad.push("Eco-Score E".to_string()); }
        _ => {}
    }

    score = score.clamp(0, 100);
    (score as u8, good, bad)
}

#[component]
fn ScanView(
    stores: RwSignal<Vec<Store>>,
    products: RwSignal<Vec<Product>>,
    entries: RwSignal<Vec<PriceEntry>>,
    nav: RwSignal<Nav>,
    settings: RwSignal<Settings>,
) -> impl IntoView {
    let _ = nav;
    let barcode = RwSignal::new(String::new());
    let status = RwSignal::new(String::new());
    let result: RwSignal<Option<ScanResult>> = RwSignal::new(None);
    let selected_store: RwSignal<String> = RwSignal::new(String::new());
    let new_store_name = RwSignal::new(String::new());
    let add_price = RwSignal::new(String::new());
    let add_qty = RwSignal::new("1".to_string());
    let scanning = RwSignal::new(false);

    // Keep scan callbacks alive while scanning.
    let scan_result_cb = StoredValue::new_local(None::<Closure<dyn FnMut(String)>>);
    let scan_error_cb = StoredValue::new_local(None::<Closure<dyn FnMut(String)>>);

    let start_scan = move |_| {
        if scanning.get_untracked() {
            return;
        }
        status.set("Starting camera…".to_string());
        let barcode = barcode.clone();
        let status = status.clone();
        let scanning = scanning.clone();

        let on_res = Closure::wrap(Box::new(move |code: String| {
            barcode.set(code.clone());
            status.set(format!("Scanned: {}. Tap Look up.", code));
            scanning.set(false);
            // Trigger lookup automatically by setting status; user can tap Look up. Auto-lookup:
            // (can't easily call lookup closure here without clone complexity; keep simple)
        }) as Box<dyn FnMut(String)>);
        let on_err = Closure::wrap(Box::new(move |msg: String| {
            status.set(msg);
            scanning.set(false);
        }) as Box<dyn FnMut(String)>);

        let on_res_fn = on_res.as_ref().unchecked_ref::<js_sys::Function>().clone();
        let on_err_fn = on_err.as_ref().unchecked_ref::<js_sys::Function>().clone();
        let _promise = js_start_barcode_scan(&on_res_fn, &on_err_fn);
        // startBarcodeScan returns a Promise<bool>; we optimistically show scanning UI
        // and let the callbacks update state (success -> scanning false, error -> scanning false).
        scanning.set(true);
        scan_result_cb.set_value(Some(on_res));
        scan_error_cb.set_value(Some(on_err));
    };

    let stop_scan = move |_| {
        js_stop_barcode_scan();
        scanning.set(false);
        status.set("Camera stopped".to_string());
        scan_result_cb.set_value(None);
        scan_error_cb.set_value(None);
    };

    let lookup = move |_| {
        let code = barcode.get_untracked().trim().to_string();
        if code.is_empty() {
            status.set("Enter a barcode number".to_string());
            return;
        }
        status.set("Looking up product…".to_string());
        result.set(None);
        let status = status.clone();
        let result = result.clone();
        spawn_local(async move {
            let url = format!(
                "https://world.openfoodfacts.org/api/v2/product/{}.json?fields=product_name,brands,nutriscore_grade,nova_group,additives_tags,ingredients_analysis_tags,ingredients_text,ecoscore_grade,quantity",
                code
            );
            match gloo_net::http::Request::get(&url).send().await {
                Ok(resp) => match resp.json::<serde_json::Value>().await {
                    Ok(v) => {
                        let found = v.get("status").and_then(|s| s.as_i64()).unwrap_or(0) == 1;
                        if !found {
                            status.set("Product not found on Open Food Facts. You can add it manually from a store.".to_string());
                            return;
                        }
                        let p = v.get("product").cloned().unwrap_or_default();
                        let name = p.get("product_name").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
                        let brand = p.get("brands").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
                        let qty = p.get("quantity").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
                        let nutri = p.get("nutriscore_grade").and_then(|x| x.as_str()).unwrap_or("unknown").to_string();
                        let nova = p.get("nova_group").and_then(|x| x.as_i64());
                        let eco = p.get("ecoscore_grade").and_then(|x| x.as_str()).unwrap_or("").to_string();
                        let additives_n = p.get("additives_tags").and_then(|x| x.as_array()).map(|a| a.len()).unwrap_or(0);
                        let (grade, mut good, mut bad) = compute_grade(&nutri, nova, additives_n, &eco);
                        if let Some(tags) = p.get("ingredients_analysis_tags").and_then(|x| x.as_array()) {
                            for t in tags.iter().filter_map(|x| x.as_str()) {
                                if t.contains("palm-oil") {
                                    if t.contains("palm-oil-free") { good.push("Palm oil free".to_string()); }
                                    else if !t.contains("may-contain") { bad.push("Contains palm oil".to_string()); }
                                }
                                if t.contains("vegan") && !t.contains("non-vegan") { good.push("Vegan".to_string()); }
                                if t.contains("vegetarian") && !t.contains("non-vegetarian") { good.push("Vegetarian".to_string()); }
                            }
                        }
                        if let Some(ing) = p.get("ingredients_text").and_then(|x| x.as_str()) {
                            let ing = ing.trim();
                            if !ing.is_empty() && ing.len() < 180 {
                                good.push(format!("Ingredients: {}", ing));
                            } else if !ing.is_empty() {
                                let mut end = 160.min(ing.len());
                                while end > 0 && !ing.is_char_boundary(end) { end -= 1; }
                                good.push(format!("Ingredients: {}…", &ing[..end]));
                            }
                        }
                        let display = if name.is_empty() { format!("Product {}", code) } else { name.clone() };
                        result.set(Some(ScanResult {
                            barcode: code.clone(),
                            name: display,
                            brand,
                            quantity: qty,
                            grade,
                            nutriscore: nutri.to_uppercase(),
                            nova: nova.map(|n| n.to_string()).unwrap_or_default(),
                            good,
                            bad,
                        }));
                        status.set("Found. Review and add to a store.".to_string());
                    }
                    Err(_) => status.set("Could not read product data".to_string()),
                },
                Err(_) => status.set("Network error while fetching product".to_string()),
            }
        });
    };

    let add_to_store = move |_| {
        haptic();
        let Some(res) = result.get_untracked() else {
            status.set("Look up a product first".to_string());
            return;
        };
        let sel = selected_store.get_untracked();
        let sid: u64 = if sel == "__new__" {
            let name = new_store_name.get_untracked().trim().to_string();
            if name.is_empty() {
                status.set("Enter a new store name".to_string());
                return;
            }
            let id = next_id(stores.get_untracked().iter().map(|s| s.id));
            stores.update(|v| v.push(Store { id, name, address: None }));
            id
        } else {
            match sel.parse() {
                Ok(v) => v,
                Err(_) => { status.set("Pick a store first (or create a new one)".to_string()); return; }
            }
        };
        // find or create product by barcode
        let pid = products.get_untracked().iter()
            .find(|p| p.barcode.as_deref() == Some(res.barcode.as_str()))
            .map(|p| p.id)
            .unwrap_or_else(|| {
                let id = next_id(products.get_untracked().iter().map(|p| p.id));
                let category = domain::guess_category(&res.name).to_string();
                products.update(|v| v.push(Product {
                    id,
                    name: res.name.clone(),
                    category,
                    barcode: Some(res.barcode.clone()),
                    grade: Some(res.grade),
                    bad_ingredients: res.bad.clone(),
                    good_ingredients: res.good.clone(),
                }));
                id
            });
        // update grade/info on existing matched product
        products.update(|v| {
            if let Some(p) = v.iter_mut().find(|p| p.id == pid) {
                p.barcode = Some(res.barcode.clone());
                p.grade = Some(res.grade);
                p.bad_ingredients = res.bad.clone();
                p.good_ingredients = res.good.clone();
                if p.name.trim().is_empty() { p.name = res.name.clone(); }
            }
        });
        // create a price entry in the selected store
        let eid = next_id(entries.get_untracked().iter().map(|e| e.id));
        let mut entry = PriceEntry::new(eid, sid, pid);
        let price = add_price.get_untracked();
        entry.price = if price.trim().is_empty() { "0".to_string() } else { price };
        entry.quantity = add_qty.get_untracked();
        entry.qty_unit = "pc".to_string();
        entry.currency = settings.get_untracked().main_currency;
        entry.date = today_iso();
        entries.update(|v| v.push(entry));
        status.set("Added to store".to_string());
    };

    view! {
        <div class="page">
            <header><h1>"Scan & rate"</h1></header>
            <div class="hint">"Enter a barcode number to look up a product (Open Food Facts). You’ll see a grade /100, good/bad ingredients, and can add it to a store."</div>

            <div class="card">
                <label class="field">
                    <span class="lbl">"Barcode (EAN/UPC)"</span>
                    <input
                        inputmode="numeric"
                        placeholder="e.g. 3017620422003"
                        prop:value=move || barcode.get()
                        on:input=move |ev| barcode.set(event_target_value(&ev))
                    />
                </label>
                <div class="row" style="gap:8px;">
                    <button class="add-btn" style="flex:1; margin-top:8px;" on:click=lookup>"Look up"</button>
                    <Show
                        when=move || scanning.get()
                        fallback=move || view! { <button class="add-btn" style="flex:1; margin-top:8px;" on:click=move |ev| { haptic(); start_scan(ev); }>"Use camera"</button> }
                    >
                        <button class="add-btn danger" style="flex:1; margin-top:8px; background:#1a0a0a; border-color:#844;" on:click=stop_scan>"Stop camera"</button>
                    </Show>
                </div>
                <Show when=move || scanning.get()>
                    <div id="scan-video-host" style="margin-top:10px;"></div>
                    <div class="hint" style="margin-top:8px;">"Point the camera at a barcode. If it doesn’t scan, type the number above."</div>
                </Show>
            </div>

            <Show when=move || !status.get().is_empty()>
                <div class="hint">{move || status.get()}</div>
            </Show>

            <Show when=move || result.get().is_some()>
                {move || result.get().map(|r| {
                    let g = r.grade;
                    let cls = if g >= 75 { "grade good" } else if g >= 40 { "grade mid" } else { "grade bad" };
                    let name = r.name.clone();
                    let sub = if r.brand.is_empty() { r.quantity.clone() } else { format!("{} · {}", r.brand, r.quantity) };
                    let meta = format!("Nutri-Score {} · NOVA {}", r.nutriscore, if r.nova.is_empty() { "?".into() } else { r.nova.clone() });
                    let good = r.good.clone();
                    let bad = r.bad.clone();
                    let has_good = !good.is_empty();
                    let has_bad = !bad.is_empty();
                    view! {
                        <div class="card">
                            <div class="row" style="justify-content:space-between; align-items:center; gap:12px;">
                                <div>
                                    <div class="s-name">{name}</div>
                                    <div class="s-sub">{sub}</div>
                                    <div class="s-sub">{meta}</div>
                                </div>
                                <div class=cls>{format!("{}/100", g)}</div>
                            </div>
                            <Show when=move || has_good>
                                <div style="margin-top:10px;">
                                    <div class="eh-title">"Good / positives"</div>
                                    <ul class="ing-list good">{good.iter().map(|x| view!{ <li>{x.clone()}</li> }).collect_view()}</ul>
                                </div>
                            </Show>
                            <Show when=move || has_bad>
                                <div style="margin-top:8px;">
                                    <div class="eh-title">"Bad / watch out"</div>
                                    <ul class="ing-list bad">{bad.iter().map(|x| view!{ <li>{x.clone()}</li> }).collect_view()}</ul>
                                </div>
                            </Show>

                            <h2 class="section" style="margin-top:12px;">"Add to a store"</h2>
                            <label class="field">
                                <span class="lbl">"Store"</span>
                                <select class="unit-select wide" prop:value=move || selected_store.get() on:change=move |ev| selected_store.set(event_target_value(&ev))>
                                    <option value="">"Select a store…"</option>
                                    <option value="__new__">"New store…"</option>
                                    {move || stores.get().iter().map(|s| view!{ <option value=s.id.to_string()>{s.name.clone()}</option> }).collect_view()}
                                </select>
                            </label>
                            <Show when=move || selected_store.get() == "__new__">
                                <label class="field">
                                    <span class="lbl">"New store name"</span>
                                    <input placeholder="Store name" prop:value=move || new_store_name.get() on:input=move |ev| new_store_name.set(event_target_value(&ev)) />
                                </label>
                            </Show>
                            <div class="grid2">
                                <label class="field">
                                    <span class="lbl">"Price (optional)"</span>
                                    <input inputmode="decimal" placeholder="0.00" prop:value=move || add_price.get() on:input=move |ev| add_price.set(event_target_value(&ev)) />
                                </label>
                                <label class="field">
                                    <span class="lbl">"Quantity"</span>
                                    <input inputmode="decimal" placeholder="1" prop:value=move || add_qty.get() on:input=move |ev| add_qty.set(event_target_value(&ev)) />
                                </label>
                            </div>
                            <button class="add-btn" on:click=add_to_store>"Add product to store"</button>
                        </div>
                    }
                })}
            </Show>
        </div>
    }
}

// ---------------------------------------------------------------- settings view

#[component]
fn SettingsView(
    settings: RwSignal<Settings>,
    currencies: RwSignal<Vec<Currency>>,
    stores: RwSignal<Vec<Store>>,
    products: RwSignal<Vec<Product>>,
    entries: RwSignal<Vec<PriceEntry>>,
) -> impl IntoView {
    let status = RwSignal::new(String::new());
    let adding = RwSignal::new(false);
    let new_code = RwSignal::new(String::new());
    let new_symbol = RwSignal::new(String::new());
    let new_rate = RwSignal::new(String::new());

    let main = move || settings.get().main_currency;

    let add_currency = move |_| {
        haptic();
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
            <header><h1>{move || tr(&settings.get().language, "settings").to_string()}</h1></header>

            <h2 class="section">{move || tr(&settings.get().language, "language").to_string()}</h2>
            <label class="field">
                <span class="lbl">{move || tr(&settings.get().language, "app_language").to_string()}</span>
                <select
                    class="unit-select wide"
                    prop:value=move || settings.get().language.clone()
                    on:change=move |ev| {
                        settings.update(|s| s.language = event_target_value(&ev));
                    }
                >
                    <option value="en">"English"</option>
                    <option value="fr">"Français"</option>
                    <option value="es">"Español"</option>
                    <option value="zh">"中文"</option>
                </select>
            </label>

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

            <h2 class="section">"Second currency"</h2>
            <div class="hint">
                "Optional — converted prices are also shown in this currency, right next to the main one. Useful to keep a feel for prices from back home (e.g. see EUR while shopping in AUD)."
            </div>
            <select
                class="unit-select wide main-select"
                prop:value=move || settings.get().second_currency.clone().unwrap_or_default()
                on:change=move |ev| {
                    let v = event_target_value(&ev);
                    settings.update(|s| {
                        s.second_currency = if v.is_empty() { None } else { Some(v) };
                    });
                }
            >
                <option value="">"None"</option>
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

            <button class="add-btn" on:click=move |_| { haptic(); fetch_live(()); }>"Fetch live rates"</button>
            <Show when=move || !status.get().is_empty()>
                <div class="hint">{move || status.get()}</div>
            </Show>

            <h2 class="section">"Add a currency"</h2>
            <Show
                when=move || adding.get()
                fallback=move || {
                    view! {
                        <button class="collapse-btn" on:click=move |_| adding.set(true)>
                            "Add a currency"
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

            <h2 class="section">"Export / Import data"</h2>
            <div class="hint">
                "Move your data between devices (iPhone ↔ Mac). Export creates a backup you can copy/save; Import restores it on this device (replaces current data)."
            </div>
            {move || {
                let export_json = move |_| {
                    let data = serde_json::json!({
                        "stores": stores.get_untracked(),
                        "products": products.get_untracked(),
                        "entries": entries.get_untracked(),
                        "currencies": currencies.get_untracked(),
                        "settings": settings.get_untracked(),
                    });
                    match serde_json::to_string_pretty(&data) {
                        Ok(s) => status.set(format!("EXPORT_OK\n{}", s)),
                        Err(_) => status.set("Export failed".to_string()),
                    }
                };
                let import_json = move |_| {
                    let txt = status.get_untracked();
                    // Accept either raw JSON or the "EXPORT_OK\n..." form
                    let payload = txt.strip_prefix("EXPORT_OK\n").unwrap_or(&txt).trim().to_string();
                    if payload.is_empty() {
                        status.set("Paste JSON backup below, then tap Import".to_string());
                        return;
                    }
                    let v: Result<serde_json::Value, _> = serde_json::from_str(&payload);
                    match v {
                        Ok(val) => {
                            if let Ok(s) = serde_json::from_value(val.get("stores").cloned().unwrap_or_default()) { stores.set(s); }
                            if let Ok(p) = serde_json::from_value(val.get("products").cloned().unwrap_or_default()) { products.set(p); }
                            if let Ok(e) = serde_json::from_value(val.get("entries").cloned().unwrap_or_default()) { entries.set(e); }
                            if let Ok(c) = serde_json::from_value(val.get("currencies").cloned().unwrap_or_default()) { currencies.set(c); }
                            if let Ok(s) = serde_json::from_value(val.get("settings").cloned().unwrap_or_default()) { settings.set(s); }
                            status.set("Import complete ✓".to_string());
                        }
                        Err(_) => status.set("Invalid JSON — paste a valid backup".to_string()),
                    }
                };
                view! {
                    <div class="row" style="gap:8px; margin-bottom:8px;">
                        <button class="add-btn" style="margin-top:0" on:click=export_json>"Export (show JSON)"</button>
                        <button class="add-btn" style="margin-top:0" on:click=import_json>"Import (from text)"</button>
                    </div>
                    <textarea
                        class="search-input"
                        style="min-height:140px; font-size:12px; font-family: ui-monospace, monospace;"
                        placeholder="Tap Export to generate a backup here, then copy it (or paste a backup here and tap Import)."
                        prop:value=move || status.get()
                        on:input=move |ev| status.set(event_target_value(&ev))
                    ></textarea>
                }
            }}

            <footer>
                <p>"All data stays on this device (localStorage). Nothing is sent anywhere, except live-rate fetches."</p>
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
            <button class="scan-fab" title="Scan" on:click=move |_| { haptic(); nav.set(Nav::Scan); }>"Scan"</button>
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
                Nav::Scan => view! { <ScanView stores products entries nav settings /> }.into_any(),
                Nav::Settings => view! { <SettingsView settings currencies stores products entries /> }.into_any(),
            }}
            <TabBar nav settings />
        </main>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
