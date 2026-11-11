//! Price Compare — compare fruit/vegetable prices across stores.
//! Black & white OLED-friendly PWA built with Leptos (CSR).

mod storage;

use leptos::prelude::*;
use leptos::task::spawn_local;
use useful_app::domain::{self, Offer};
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

// ---------------------------------------------------------------- ranking

#[derive(Clone, Debug, PartialEq)]
struct RankItem {
    id: u64,
    store: String,
    address: Option<String>,
    promo: bool,
    date: String,
    per_kg: Option<f64>,
    per_unit: Option<f64>,
    /// How much more expensive per kg than the winner (0 for the winner).
    delta: Option<f64>,
    best: bool,
}

fn compute_ranking(items: &[Offer]) -> Vec<RankItem> {
    let quotes: Vec<domain::Quote> = items.iter().map(domain::quote).collect();

    let mut rows: Vec<RankItem> = items
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let q = quotes[i];
            RankItem {
                id: o.id,
                store: if o.store.trim().is_empty() {
                    format!("Store {}", i + 1)
                } else {
                    o.store.trim().to_string()
                },
                address: o.address.clone().filter(|a| !a.trim().is_empty()),
                promo: o.promo,
                date: o.date.clone(),
                per_kg: q.per_kg,
                per_unit: q.per_unit,
                delta: None,
                best: false,
            }
        })
        .collect();

    // Comparable offers (price per kg known) first, cheapest on top.
    rows.sort_by(|a, b| match (a.per_kg, b.per_kg) {
        (Some(x), Some(y)) => x
            .partial_cmp(&y)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    let min = match domain::best(&quotes) {
        domain::Best::Winner(i) | domain::Best::Tied(i) => quotes[i].per_kg,
        domain::Best::None => None,
    };

    if let Some(min) = min {
        for r in rows.iter_mut() {
            if let Some(k) = r.per_kg {
                r.delta = Some(k - min);
                r.best = (k - min).abs() <= min * 1e-6;
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

/// Ask the browser for the current position and write the resolved
/// address into the offer with the given id.
fn pin_address(
    offers: RwSignal<Vec<RwSignal<Offer>>>,
    id: u64,
    pinning: RwSignal<bool>,
) {
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
            offers.update(|v| {
                if let Some(sig) = v.iter().find(|s| s.get_untracked().id == id) {
                    sig.update(|o| o.address = Some(addr));
                }
            });
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

// ---------------------------------------------------------------- offer card

#[component]
fn OfferCard(
    offer: RwSignal<Offer>,
    offers: RwSignal<Vec<RwSignal<Offer>>>,
) -> impl IntoView {
    let pinning = RwSignal::new(false);
    let id = offer.get_untracked().id;

    let remove = move |_| {
        offers.update(|v| v.retain(|s| s.get_untracked().id != id));
    };
    let pin = move |_| pin_address(offers, id, pinning);

    let set_store = move |v: String| offer.update(|o| o.store = v);
    let set_address = move |v: String| offer.update(|o| o.address = Some(v));
    let set_kg_price = move |v: String| offer.update(|o| o.kg_price = v);
    let set_unit_price = move |v: String| offer.update(|o| o.unit_price = v);
    let set_weight = move |v: String| offer.update(|o| o.weight = v);
    let set_weight_unit = move |v: String| offer.update(|o| o.weight_unit = v);
    let set_date = move |v: String| offer.update(|o| o.date = v);
    let set_promo = move |v: bool| offer.update(|o| o.promo = v);

    view! {
        <div class="card">
            <div class="row">
                <input
                    class="store-input"
                    placeholder="Store name"
                    prop:value=move || offer.get().store
                    on:input=move |ev| set_store(event_target_value(&ev))
                />
                <button class="icon-btn" title="Remove this store" on:click=remove>
                    "✕"
                </button>
            </div>

            <div class="row">
                <input
                    class="addr-input"
                    placeholder="Address (optional)"
                    prop:value=move || offer.get().address.clone().unwrap_or_default()
                    on:input=move |ev| set_address(event_target_value(&ev))
                />
                <button class="icon-btn" title="Use my current location" on:click=pin>
                    "📍"
                </button>
            </div>
            <Show when=move || pinning.get()>
                <div class="hint">"Locating…"</div>
            </Show>

            <div class="grid3">
                <label class="field">
                    <span class="lbl">"Price / kg"</span>
                    <input
                        inputmode="decimal"
                        placeholder="4.99"
                        prop:value=move || offer.get().kg_price
                        on:input=move |ev| set_kg_price(event_target_value(&ev))
                    />
                </label>
                <label class="field">
                    <span class="lbl">"Price / unit"</span>
                    <input
                        inputmode="decimal"
                        placeholder="0.89"
                        prop:value=move || offer.get().unit_price
                        on:input=move |ev| set_unit_price(event_target_value(&ev))
                    />
                </label>
                <label class="field">
                    <span class="lbl">"Weight"</span>
                    <div class="weight-row">
                        <input
                            inputmode="decimal"
                            placeholder="200"
                            prop:value=move || offer.get().weight
                            on:input=move |ev| set_weight(event_target_value(&ev))
                        />
                        <select
                            class="unit-select"
                            prop:value=move || offer.get().weight_unit
                            on:change=move |ev| set_weight_unit(event_target_value(&ev))
                        >
                            <option value="g">"g"</option>
                            <option value="kg">"kg"</option>
                            <option value="oz">"oz"</option>
                            <option value="lb">"lb"</option>
                        </select>
                    </div>
                </label>
            </div>

            <div class="row meta-row">
                <label class="field date-field">
                    <span class="lbl">"Found on"</span>
                    <input
                        type="date"
                        prop:value=move || offer.get().date
                        on:input=move |ev| set_date(event_target_value(&ev))
                    />
                </label>
                <label class="check">
                    <input
                        type="checkbox"
                        prop:checked=move || offer.get().promo
                        on:change=move |ev| set_promo(event_target_checked(&ev))
                    />
                    <span>"Promo"</span>
                </label>
            </div>

            {move || {
                let q = domain::quote(&offer.get());
                let text = match (q.per_kg, q.per_unit) {
                    (None, None) => None,
                    (kg, unit) => {
                        let mut parts = Vec::new();
                        if let Some(k) = kg {
                            parts.push(format!("{} /kg", fmt_price(k)));
                        }
                        if let Some(u) = unit {
                            parts.push(format!("{} each", fmt_price(u)));
                        }
                        Some(parts.join("  ·  "))
                    }
                };
                text.map(|text| view! { <div class="quote-hint">{text}</div> })
            }}
        </div>
    }
}

// ---------------------------------------------------------------- app

fn next_id(offers: &Vec<RwSignal<Offer>>) -> u64 {
    offers
        .iter()
        .map(|s| s.get_untracked().id)
        .max()
        .unwrap_or(0)
        + 1
}

#[component]
fn App() -> impl IntoView {
    let offers: RwSignal<Vec<RwSignal<Offer>>> = RwSignal::new(
        storage::load_offers()
            .into_iter()
            .map(|o| RwSignal::new(o))
            .collect(),
    );
    let product: RwSignal<String> = RwSignal::new(storage::load_product());

    // Persist on every change.
    Effect::new(move |_| {
        let current: Vec<Offer> = offers.get().iter().map(|s| s.get()).collect();
        storage::save_offers(&current);
    });
    Effect::new(move |_| {
        storage::save_product(&product.get());
    });

    let add_store = move |_| {
        offers.update(|v| {
            let id = next_id(v);
            let mut o = Offer::new(id);
            o.date = today_iso();
            v.push(RwSignal::new(o));
        });
    };

    let clear_all = move |_| {
        offers.set(Vec::new());
    };

    let ranking = Memo::new(move |_| {
        let items: Vec<Offer> = offers.get().iter().map(|s| s.get()).collect();
        compute_ranking(&items)
    });

    let verdict = Memo::new(move |_| {
        ranking
            .get()
            .into_iter()
            .find(|r| r.per_kg.is_some())
    });

    let max_price_kg = Memo::new(move |_| {
        ranking
            .get()
            .iter()
            .filter_map(|r| r.per_kg)
            .fold(f64::NAN, f64::max)
    });

    view! {
        <main>
            <header>
                <h1>"🧺 Price Compare"</h1>
            </header>

            <input
                class="product-input"
                placeholder="What are you comparing? (e.g. Apples)"
                prop:value=move || product.get()
                on:input=move |ev| product.set(event_target_value(&ev))
            />

            // ---- verdict
            <Show when=move || verdict.get().is_some()>
                <div class="verdict">
                    <div class="v-label">"BEST DEAL"</div>
                    <div class="v-store">
                        {move || verdict.get().map(|r| r.store.clone()).unwrap_or_default()}
                    </div>
                    <div class="v-price">
                        {move || {
                            verdict.get()
                                .and_then(|r| r.per_kg)
                                .map(fmt_price)
                        }}
                        <span class="v-unit">" /kg"</span>
                    </div>
                    {move || {
                        verdict.get().and_then(|r| r.per_unit).map(|u| {
                            view! { <div class="v-sub">{fmt_price(u)}" each"</div> }
                        })
                    }}
                    {move || {
                        let max = max_price_kg.get();
                        let best = verdict.get().and_then(|r| r.per_kg);
                        if let Some(best) = best {
                            if max.is_finite() {
                                let save = max - best;
                                if save > 1e-6 {
                                    return Some(view! {
                                        <div class="v-save">
                                            {"Save "}{fmt_price(save)}{" /kg vs the priciest store"}
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
                ranking.get().iter().all(|r| r.per_kg.is_none()) && !offers.get().is_empty()
            }>
                <div class="hint">
                    "Enter a price per kg, or a price per unit plus the weight of one unit — that is what makes the comparison fair."
                </div>
            </Show>

            // ---- ranking list
            <For
                each=move || ranking.get()
                key=|r| r.id
                children=move |r: RankItem| {
                    let cls = if r.best { "rank-row best" } else { "rank-row" };
                    view! {
                        <div class=cls>
                            <div class="rank-info">
                                <div class="rank-store">
                                    {move || {
                                        let name = r.store.clone();
                                        match r.address.clone() {
                                            Some(addr) => {
                                                let url =
                                                    format!("https://maps.apple.com/?q={}", percent_encode(&addr));
                                                view! {
                                                    <a class="maps-link" href=url target="_blank">{name}</a>
                                                }.into_any()
                                            }
                                            None => {
                                                view! { <span>{name}</span> }.into_any()
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
                                    r.per_kg.map(|k| {
                                        view! { <div class="rank-kg">{fmt_price(k)}" /kg"</div> }
                                    })
                                }}
                                {move || {
                                    r.per_unit.map(|u| {
                                        view! { <div class="rank-unit">{fmt_price(u)}" each"</div> }
                                    })
                                }}
                                {move || {
                                    let data = if r.best {
                                        Some(("rank-delta winner", "★ best".to_string()))
                                    } else if r.per_kg.is_none() {
                                        Some(("rank-delta", "add weight or /kg price".to_string()))
                                    } else {
                                        r.delta
                                            .filter(|d| *d > 1e-6)
                                            .map(|d| ("rank-delta", format!("+{} /kg", fmt_price(d))))
                                    };
                                    data.map(|(cls, text)| {
                                        view! { <div class=cls>{text}</div> }
                                    })
                                }}
                            </div>
                        </div>
                    }
                }
            />

            // ---- offer input cards
            <For
                each=move || offers.get()
                key=|sig| sig.get_untracked().id
                children={move |offer: RwSignal<Offer>| {
                    view! { <OfferCard offer offers /> }
                }}
            />

            <button class="add-btn" on:click=add_store>"+ Add a store"</button>

            <Show when=move || !offers.get().is_empty()>
                <button class="clear-btn" on:click=clear_all>"Clear all"</button>
            </Show>

            <footer>
                <p>"Prices are temporary — date and promo flag help you remember when each price was found."</p>
                <p>"Data stays on this device (localStorage). Nothing is sent anywhere."</p>
            </footer>
        </main>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
