//! Pure domain logic for comparing fruit & vegetable prices across stores.
//! No WASM/browser dependencies here so it can be unit-tested natively.

use serde::{Deserialize, Serialize};

/// One store's offer for the same product.
/// Numeric fields are kept as raw strings because they are bound to text inputs;
/// use `parse_number` / `quote` to interpret them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Offer {
    pub id: u64,
    pub store: String,
    /// Optional place address (typed or pinned via GPS).
    pub address: Option<String>,
    /// Whether the price was a promotion (prices are temporary).
    pub promo: bool,
    /// Date the price was found, as YYYY-MM-DD.
    pub date: String,
    /// Price for one unit (one apple, one bunch, one pack...).
    pub unit_price: String,
    /// Price per kilogram.
    pub kg_price: String,
    /// Weight of one unit (numeric string).
    pub weight: String,
    /// Unit for `weight`: "g", "kg", "oz" or "lb".
    pub weight_unit: String,
}

impl Offer {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            store: String::new(),
            address: None,
            promo: false,
            date: String::new(),
            unit_price: String::new(),
            kg_price: String::new(),
            weight: String::new(),
            weight_unit: "g".to_string(),
        }
    }
}

/// Parse a user-typed numeric value.
/// Accepts "1.29", "1,29", "$1.29", " 2 " etc. Returns None for empty/invalid/<= 0.
pub fn parse_number(input: &str) -> Option<f64> {
    let cleaned: String = input
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
        .collect();
    let normalized = cleaned.replace(',', ".");
    // More than one decimal separator means we cannot guess the intent.
    if normalized.matches('.').count() > 1 {
        return None;
    }
    normalized
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
}

/// Convert a weight value + unit into kilograms.
pub fn weight_to_kg(value: f64, unit: &str) -> f64 {
    match unit {
        "kg" => value,
        "lb" => value * 0.45359237,
        "oz" => value * 0.028349523125,
        // default is grams
        _ => value / 1000.0,
    }
}

/// Normalized prices computed from an offer's inputs.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Quote {
    /// Price for one unit.
    pub per_unit: Option<f64>,
    /// Price per kilogram — the canonical comparison basis.
    pub per_kg: Option<f64>,
}

/// Compute the normalized price of an offer.
///
/// Rules:
/// - per_kg: taken from the kg price if given, otherwise derived as
///   unit_price / unit_weight when both are available.
/// - per_unit: taken from the unit price if given, otherwise derived as
///   kg_price * unit_weight when both are available.
pub fn quote(offer: &Offer) -> Quote {
    let unit_price = parse_number(&offer.unit_price);
    let kg_price = parse_number(&offer.kg_price);
    let weight_kg = parse_number(&offer.weight).map(|w| weight_to_kg(w, &offer.weight_unit));

    let per_kg = kg_price.or_else(|| match (unit_price, weight_kg) {
        (Some(p), Some(w)) if w > 0.0 => Some(p / w),
        _ => None,
    });

    let per_unit = unit_price.or_else(|| match (kg_price, weight_kg) {
        (Some(k), Some(w)) if w > 0.0 => Some(k * w),
        _ => None,
    });

    Quote { per_unit, per_kg }
}

/// Result of comparing all offers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Best {
    /// Index of the single cheapest offer (by price per kg).
    Winner(usize),
    /// Index of one of several equally cheap offers.
    Tied(usize),
    /// No offer can be normalized to a price per kg.
    None,
}

/// Pick the best offer by normalized price per kg.
pub fn best(quotes: &[Quote]) -> Best {
    let comparable: Vec<(usize, f64)> = quotes
        .iter()
        .enumerate()
        .filter_map(|(i, q)| q.per_kg.map(|k| (i, k)))
        .collect();

    if comparable.is_empty() {
        return Best::None;
    }

    let min = comparable
        .iter()
        .map(|(_, k)| *k)
        .fold(f64::INFINITY, f64::min);

    let winners: Vec<usize> = comparable
        .iter()
        .filter(|(_, k)| (*k - min).abs() <= min * 1e-6)
        .map(|(i, _)| *i)
        .collect();

    match winners.len() {
        1 => Best::Winner(winners[0]),
        _ => Best::Tied(winners[0]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(unit_price: &str, kg_price: &str, weight: &str, unit: &str) -> Offer {
        Offer {
            id: 0,
            store: "test".into(),
            address: None,
            promo: false,
            date: "2026-11-11".into(),
            unit_price: unit_price.into(),
            kg_price: kg_price.into(),
            weight: weight.into(),
            weight_unit: unit.into(),
        }
    }

    #[test]
    fn unit_only_with_weight_derives_per_kg() {
        // 0.50 per apple, one apple weighs 200 g -> 2.50 /kg
        let q = quote(&offer("0.50", "", "200", "g"));
        assert_eq!(q.per_unit, Some(0.50));
        assert!((q.per_kg.unwrap() - 2.5).abs() < 1e-9);
    }

    #[test]
    fn kg_only_with_weight_derives_per_unit() {
        // 2.99 /kg, one unit weighs 500 g -> 1.495 each
        let q = quote(&offer("", "2.99", "500", "g"));
        assert_eq!(q.per_kg, Some(2.99));
        assert!((q.per_unit.unwrap() - 1.495).abs() < 1e-9);
    }

    #[test]
    fn kg_price_wins_when_both_given() {
        let q = quote(&offer("0.50", "3.00", "200", "g"));
        assert_eq!(q.per_kg, Some(3.00));
        assert_eq!(q.per_unit, Some(0.50));
    }

    #[test]
    fn unit_price_without_weight_is_not_comparable() {
        let q = quote(&offer("0.50", "", "", ""));
        assert_eq!(q.per_unit, Some(0.50));
        assert_eq!(q.per_kg, None);
    }

    #[test]
    fn winner_is_cheapest_per_kg() {
        // A: 0.50/unit @ 200 g -> 2.50 /kg ; B: 2.99 /kg ; C: 2.40 /kg
        let offers = vec![
            offer("0.50", "", "200", "g"),
            offer("", "2.99", "", ""),
            offer("", "2.40", "", ""),
        ];
        let quotes: Vec<Quote> = offers.iter().map(quote).collect();
        assert_eq!(best(&quotes), Best::Winner(2));
    }

    #[test]
    fn tied_offers() {
        let offers = vec![offer("", "3.00", "", ""), offer("", "3.00", "", "")];
        let quotes: Vec<Quote> = offers.iter().map(quote).collect();
        assert_eq!(best(&quotes), Best::Tied(0));
    }

    #[test]
    fn nothing_comparable() {
        let offers = vec![offer("0.50", "", "", ""), offer("0.60", "", "", "")];
        let quotes: Vec<Quote> = offers.iter().map(quote).collect();
        assert_eq!(best(&quotes), Best::None);
    }

    #[test]
    fn parses_comma_and_currency() {
        assert_eq!(parse_number("1,29"), Some(1.29));
        assert_eq!(parse_number("$1.29"), Some(1.29));
        assert_eq!(parse_number(" 2,49 € "), Some(2.49));
        assert_eq!(parse_number(""), None);
        assert_eq!(parse_number("abc"), None);
        assert_eq!(parse_number("0"), None);
        assert_eq!(parse_number("1.2.3"), None);
    }

    #[test]
    fn weight_units_to_kg() {
        assert!((weight_to_kg(1000.0, "g") - 1.0).abs() < 1e-9);
        assert!((weight_to_kg(1.0, "kg") - 1.0).abs() < 1e-9);
        assert!((weight_to_kg(16.0, "oz") - 0.45359237).abs() < 1e-6);
        assert!((weight_to_kg(1.0, "lb") - 0.45359237).abs() < 1e-9);
    }
}
