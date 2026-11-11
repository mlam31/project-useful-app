//! Persistence of offers + product name in the browser's localStorage.

use useful_app::domain::Offer;

const OFFERS_KEY: &str = "price-compare.offers";
const PRODUCT_KEY: &str = "price-compare.product";

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

pub fn load_offers() -> Vec<Offer> {
    let Some(ls) = local_storage() else {
        return Vec::new();
    };
    ls.get_item(OFFERS_KEY)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str::<Vec<Offer>>(&json).ok())
        .unwrap_or_default()
}

pub fn save_offers(offers: &[Offer]) {
    if let Some(ls) = local_storage() {
        if let Ok(json) = serde_json::to_string(offers) {
            let _ = ls.set_item(OFFERS_KEY, &json);
        }
    }
}

pub fn load_product() -> String {
    local_storage()
        .and_then(|ls| ls.get_item(PRODUCT_KEY).ok().flatten())
        .unwrap_or_default()
}

pub fn save_product(name: &str) {
    if let Some(ls) = local_storage() {
        let _ = ls.set_item(PRODUCT_KEY, name);
    }
}
