use useful_app::domain::{best, quote, weight_to_kg, Best, Offer, Quote};

fn offer(id: u64, store: &str, unit_price: &str, kg_price: &str, weight: &str, unit: &str) -> Offer {
    Offer {
        id,
        store: store.into(),
        address: None,
        promo: false,
        date: "2026-11-11".into(),
        unit_price: unit_price.into(),
        kg_price: kg_price.into(),
        weight: weight.into(),
        weight_unit: unit.into(),
    }
}

fn quotes(offers: &[Offer]) -> Vec<Quote> {
    offers.iter().map(quote).collect()
}

/// The core scenario: one store sells per unit (weight known),
/// another sells per kg — the app must normalize and pick the winner.
#[test]
fn per_unit_store_vs_per_kg_store() {
    // A: 0.50 per apple @ 200 g -> 2.50 /kg ; B: 2.99 /kg
    let offers = vec![
        offer(1, "Green Grocer", "0.50", "", "200", "g"),
        offer(2, "SuperMart", "", "2.99", "", ""),
    ];
    assert_eq!(best(&quotes(&offers)), Best::Winner(0));
}

#[test]
fn per_kg_store_beats_expensive_per_unit_store() {
    // A: 0.40 per apple @ 150 g -> 2.67 /kg ; B: 2.40 /kg
    let offers = vec![
        offer(1, "A", "0.40", "", "150", "g"),
        offer(2, "B", "", "2.40", "", ""),
    ];
    assert_eq!(best(&quotes(&offers)), Best::Winner(1));
}

#[test]
fn equal_prices_are_tied() {
    let offers = vec![
        offer(1, "A", "", "3.00", "", ""),
        offer(2, "B", "1.50", "", "500", "g"), // 3.00 /kg
    ];
    let q = quotes(&offers);
    assert_eq!(best(&q), Best::Tied(0));
    assert!((q[0].per_kg.unwrap() - q[1].per_kg.unwrap()).abs() < 1e-6);
}

#[test]
fn offers_without_normalizable_price_are_ignored() {
    // A has no weight -> cannot be compared on /kg basis
    let offers = vec![
        offer(1, "A", "0.50", "", "", ""),
        offer(2, "B", "", "2.00", "", ""),
    ];
    assert_eq!(best(&quotes(&offers)), Best::Winner(1));
}

#[test]
fn nothing_comparable_at_all() {
    let offers = vec![
        offer(1, "A", "0.50", "", "", ""),
        offer(2, "B", "0.60", "", "", ""),
    ];
    assert_eq!(best(&quotes(&offers)), Best::None);
}

#[test]
fn comma_decimals_and_currency_symbols_parse() {
    let o = offer(1, "A", "0,50", "$2.99", "250", "g");
    let q = quote(&o);
    assert_eq!(q.per_unit, Some(0.50));
    assert_eq!(q.per_kg, Some(2.99));
    assert!((q.per_kg.unwrap() - 2.99).abs() < 1e-9);
}

#[test]
fn weight_units_convert_to_kg() {
    assert!((weight_to_kg(250.0, "g") - 0.25).abs() < 1e-9);
    assert!((weight_to_kg(1.0, "kg") - 1.0).abs() < 1e-9);
    assert!((weight_to_kg(2.0, "lb") - 0.90718474).abs() < 1e-6);
    assert!((weight_to_kg(8.0, "oz") - 0.226796185).abs() < 1e-6);
}
