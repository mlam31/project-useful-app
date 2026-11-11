//! Persistence of stores, products and price entries in localStorage.

use useful_app::domain::{PriceEntry, Product, Store};
use web_sys::Storage;

const STORES_KEY: &str = "pc.stores";
const PRODUCTS_KEY: &str = "pc.products";
const ENTRIES_KEY: &str = "pc.entries";

fn local_storage() -> Option<Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

fn load<T: serde::de::DeserializeOwned>(key: &str) -> Vec<T> {
    let Some(ls) = local_storage() else {
        return Vec::new();
    };
    ls.get_item(key)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str::<Vec<T>>(&json).ok())
        .unwrap_or_default()
}

fn save<T: serde::Serialize>(key: &str, value: &[T]) {
    if let Some(ls) = local_storage() {
        if let Ok(json) = serde_json::to_string(value) {
            let _ = ls.set_item(key, &json);
        }
    }
}

pub fn load_stores() -> Vec<Store> {
    load(STORES_KEY)
}
pub fn save_stores(stores: &[Store]) {
    save(STORES_KEY, stores);
}

pub fn load_products() -> Vec<Product> {
    load(PRODUCTS_KEY)
}
pub fn save_products(products: &[Product]) {
    save(PRODUCTS_KEY, products);
}

pub fn load_entries() -> Vec<PriceEntry> {
    load(ENTRIES_KEY)
}
pub fn save_entries(entries: &[PriceEntry]) {
    save(ENTRIES_KEY, entries);
}
