use useful_app::domain::{
    best_metric, entry_quote, latest_per_store, parse_number, weight_to_kg, Best, PriceEntry,
};

fn entry(id: u64, store: u64, price: &str, qty: &str, weight: &str, unit: &str) -> PriceEntry {
    PriceEntry {
        id,
        store_id: store,
        product_id: 1,
        price: price.into(),
        quantity: qty.into(),
        qty_unit: "pc".into(),
        unit_weight: weight.into(),
        weight_unit: unit.into(),
        currency: "EUR".into(),
        promo: false,
        promo_deal: String::new(),
        date: "2026-11-11".into(),
    }
}

/// A second product — used for the multi-product comparison test.
fn entry2(id: u64, store: u64, product: u64, price: &str, qty: &str) -> PriceEntry {
    PriceEntry {
        product_id: product,
        ..entry(id, store, price, qty, "", "")
    }
}

/// The core scenario: produce priced per unit with a known weight must be
/// comparable on a /kg basis with store prices given per kg.
#[test]
fn unit_price_with_weight_normalizes_to_per_kg() {
    // 0.50 for 1 apple of 200 g -> 2.50 /kg
    let q = entry_quote(&entry(1, 10, "0.50", "1", "200", "g"));
    assert_eq!(q.per_unit, Some(0.50));
    assert!((q.per_kg.unwrap() - 2.5).abs() < 1e-9);
}

#[test]
fn multipack_divides_per_unit() {
    // 4.99 for a 6-pack -> 0.83 each, no /kg without weight
    let q = entry_quote(&entry(1, 10, "4.99", "6", "", ""));
    assert!((q.per_unit.unwrap() - 4.99 / 6.0).abs() < 1e-9);
    assert_eq!(q.per_kg, None);
}

#[test]
fn weight_multiplies_by_quantity_for_per_kg() {
    // 4.99 for 6 units of 1 kg each -> 0.83 /kg
    let q = entry_quote(&entry(1, 10, "4.99", "6", "1", "kg"));
    assert!((q.per_kg.unwrap() - 4.99 / 6.0).abs() < 1e-9);
}

#[test]
fn best_metric_picks_lowest() {
    assert_eq!(best_metric(&[Some(2.5), Some(2.99), Some(2.4)]), Best::Winner(2));
    assert_eq!(best_metric(&[Some(3.0), None, Some(3.0)]), Best::Tied(0));
    assert_eq!(best_metric(&[None, None]), Best::None);
}

/// Older price records in the same store must not compete with the
/// current one — only the latest price per store counts.
#[test]
fn latest_per_store_keeps_newest() {
    let mut old = entry(1, 10, "2.99", "1", "", "");
    old.date = "2026-11-01".into();
    let mut new = entry(2, 10, "1.99", "1", "", "");
    new.promo = true;
    let other = entry(3, 20, "2.50", "1", "", "");

    let out = latest_per_store(&[old, new, other]);
    assert_eq!(out.len(), 2);
    assert!(out.iter().any(|e| e.id == 2));
    assert!(out.iter().any(|e| e.id == 3));
}

/// Ticking two similar products (green + yellow kiwi) must keep entries of
/// BOTH products — they are different (store, product) pairs.
#[test]
fn latest_per_store_spans_selected_products() {
    let green = entry(1, 10, "9.00", "1", "1", "kg"); // green kiwi, store 10
    let yellow = entry2(2, 10, 2, "10.00", "5"); // yellow kiwi, same store
    let out = latest_per_store(&[green, yellow]);
    assert_eq!(out.len(), 2);
    assert!(out.iter().any(|e| e.product_id == 2));
}

#[test]
fn comma_decimals_and_currency_symbols_parse() {
    assert_eq!(parse_number("1,29"), Some(1.29));
    assert_eq!(parse_number("$1.29"), Some(1.29));
    assert_eq!(parse_number(" 2,49 € "), Some(2.49));
    assert_eq!(parse_number(""), None);
    assert_eq!(parse_number("abc"), None);
    assert_eq!(parse_number("0"), None);
    assert_eq!(parse_number("1.2.3"), None);
}

#[test]
fn weight_units_convert_to_kg() {
    assert!((weight_to_kg(250.0, "g") - 0.25).abs() < 1e-9);
    assert!((weight_to_kg(1.0, "kg") - 1.0).abs() < 1e-9);
    assert!((weight_to_kg(2.0, "lb") - 0.90718474).abs() < 1e-6);
    assert!((weight_to_kg(8.0, "oz") - 0.226796185).abs() < 1e-6);
}
