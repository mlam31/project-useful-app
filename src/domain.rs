//! Pure domain logic for the price database: stores, products, price entries.
//! No WASM/browser dependencies here so it can be unit-tested natively.

use serde::{Deserialize, Serialize};

/// A place where you shop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Store {
    pub id: u64,
    pub name: String,
    /// Optional address (typed or pinned via GPS).
    pub address: Option<String>,
}

/// A product in your database (fruit, vegetable, bottle...).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Product {
    pub id: u64,
    pub name: String,
    /// One of: "fruit", "vegetable", "bottle", "other".
    pub category: String,
}

/// One price you recorded: a product, at a store, on a date.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceEntry {
    pub id: u64,
    pub store_id: u64,
    pub product_id: u64,
    /// Total price paid for `quantity` units.
    pub price: String,
    /// How many units the price covers (6-pack = "6"). Default "1".
    /// When `qty_unit` is g/kg/L/ml, this is the total amount purchased.
    pub quantity: String,
    /// What `quantity` counts: "pc" (pieces/units), "g", "kg", "l" or "ml".
    #[serde(default = "default_qty_unit")]
    pub qty_unit: String,
    /// Weight of ONE unit (optional; needed for /kg comparison). Only used
    /// when `qty_unit` is "pc".
    pub unit_weight: String,
    /// Unit for `unit_weight`: "g", "kg", "oz" or "lb".
    pub weight_unit: String,
    /// Currency code the price was recorded in (e.g. "AUD").
    pub currency: String,
    /// Prices are temporary — promo flag records it was a promotion.
    pub promo: bool,
    /// Optional promo label, e.g. "50% off", "2 for $5".
    #[serde(default)]
    pub promo_deal: String,
    /// Date the price was found, YYYY-MM-DD.
    pub date: String,
}

impl PriceEntry {
    pub fn new(id: u64, store_id: u64, product_id: u64) -> Self {
        Self {
            id,
            store_id,
            product_id,
            price: String::new(),
            quantity: "1".to_string(),
            qty_unit: "pc".to_string(),
            unit_weight: String::new(),
            weight_unit: "g".to_string(),
            currency: String::new(),
            promo: false,
            promo_deal: String::new(),
            date: String::new(),
        }
    }
}

/// A currency you track, with its exchange rate relative to your MAIN
/// currency: `rate` = how many main-currency units 1 of this currency is worth.
/// Editable manually; can also be refreshed live.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Currency {
    pub code: String,
    pub symbol: String,
    pub rate: f64,
}

/// App settings: which currency everything is compared in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub main_currency: String,
    /// Optional second currency: converted prices are also shown in it,
    /// alongside the main one (e.g. see EUR while shopping in AUD).
    #[serde(default)]
    pub second_currency: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            main_currency: "EUR".to_string(),
            second_currency: None,
        }
    }
}

/// Default quantity unit for entries stored before the field existed.
fn default_qty_unit() -> String {
    "pc".to_string()
}

/// Starter currency list. Rates are rough EUR-based placeholders —
/// edit them in Settings or fetch live rates.
pub fn default_currencies() -> Vec<Currency> {
    vec![
        Currency { code: "EUR".into(), symbol: "€".into(), rate: 1.0 },
        Currency { code: "USD".into(), symbol: "$".into(), rate: 0.92 },
        Currency { code: "AUD".into(), symbol: "A$".into(), rate: 0.60 },
        Currency { code: "JPY".into(), symbol: "¥".into(), rate: 0.0059 },
        Currency { code: "CNY".into(), symbol: "¥".into(), rate: 0.13 },
        Currency { code: "GBP".into(), symbol: "£".into(), rate: 1.17 },
    ]
}

/// Fallback symbol for a known currency code.
pub fn currency_symbol(code: &str) -> &'static str {
    match code {
        "EUR" => "€",
        "USD" | "AUD" | "NZD" | "CAD" | "SGD" => "$",
        "GBP" => "£",
        "JPY" | "CNY" => "¥",
        "CHF" => "Fr",
        _ => "¤",
    }
}

/// Guess a product category from its name (used when creating a product).
pub fn guess_category(name: &str) -> &'static str {
    let n = name.to_lowercase();
    const FRUIT: &[&str] = &[
        "apple", "banana", "orange", "kiwi", "grape", "pear", "mango", "berry", "strawberr",
        "raspberry", "blueberr", "peach", "nectarine", "plum", "melon", "watermelon", "lemon",
        "lime", "pineapple", "cherry", "fig", "apricot", "papaya", "passion", "coconut",
    ];
    const VEG: &[&str] = &[
        "cucumber", "tomato", "potato", "onion", "carrot", "lettuce", "salad", "spinach",
        "broccoli", "cauliflower", "capsicum", "pepper", "chili", "zucchini", "courgette",
        "eggplant", "aubergine", "mushroom", "garlic", "celery", "cabbage", "pumpkin",
        "squash", "corn", "beet", "radish", "asparagus", "leek", "fennel", "ginger", "herb",
        "basil", "mint", "coriander", "parsley",
    ];
    const BOTTLE: &[&str] = &[
        "bottle", "water", "soda", "cola", "juice", "milk", "beer", "wine", "drink", "tea",
        "coffee", "sparkling",
    ];
    if BOTTLE.iter().any(|k| n.contains(k)) {
        "bottle"
    } else if FRUIT.iter().any(|k| n.contains(k)) {
        "fruit"
    } else if VEG.iter().any(|k| n.contains(k)) {
        "vegetable"
    } else {
        "other"
    }
}

/// Known categories: (id, emoji, display label).
pub const CATEGORIES: [(&str, &str, &str); 4] = [
    ("fruit", "🍎", "Fruit"),
    ("vegetable", "🥬", "Veggies"),
    ("bottle", "🧴", "Drinks"),
    ("other", "📦", "Other"),
];

pub fn category_emoji(category: &str) -> &'static str {
    CATEGORIES
        .iter()
        .find(|(id, _, _)| *id == category)
        .map(|(_, e, _)| *e)
        .unwrap_or("📦")
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

/// Normalized prices computed from a price entry's inputs.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Quote {
    /// Price for one unit (total ÷ quantity). None for weighed/volume buys.
    pub per_unit: Option<f64>,
    /// Price per kilogram — the fair comparison basis for produce.
    pub per_kg: Option<f64>,
    /// Price per liter (for quantities entered in L or ml).
    pub per_l: Option<f64>,
}

/// Compute the normalized price of an entry.
///
/// - qty_unit "pc": per_unit = price / quantity, per_kg when the unit
///   weight is known
/// - qty_unit "g"/"kg": `quantity` is the total mass → per_kg
/// - qty_unit "l"/"ml": `quantity` is the total volume → per_l
pub fn entry_quote(entry: &PriceEntry) -> Quote {
    let price = parse_number(&entry.price);

    match entry.qty_unit.as_str() {
        "g" | "kg" => {
            let mass = parse_number(&entry.quantity).map(|q| weight_to_kg(q, &entry.qty_unit));
            let per_kg = match (price, mass) {
                (Some(p), Some(m)) if m > 0.0 => Some(p / m),
                _ => None,
            };
            Quote { per_unit: None, per_kg, per_l: None }
        }
        "l" | "ml" => {
            let vol = parse_number(&entry.quantity)
                .map(|v| if entry.qty_unit == "ml" { v / 1000.0 } else { v });
            let per_l = match (price, vol) {
                (Some(p), Some(v)) if v > 0.0 => Some(p / v),
                _ => None,
            };
            Quote { per_unit: None, per_kg: None, per_l }
        }
        // Pieces / units (default)
        _ => {
            let qty = parse_number(&entry.quantity).unwrap_or(1.0);
            let weight_kg =
                parse_number(&entry.unit_weight).map(|w| weight_to_kg(w, &entry.weight_unit));
            let per_unit = price.map(|p| if qty > 0.0 { p / qty } else { p });
            let per_kg = match (price, qty, weight_kg) {
                (Some(p), q, Some(w)) if q > 0.0 && w > 0.0 => Some(p / (q * w)),
                _ => None,
            };
            Quote { per_unit, per_kg, per_l: None }
        }
    }
}

/// Result of comparing several values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Best {
    /// Index of the single cheapest entry.
    Winner(usize),
    /// Index of one of several equally cheap entries.
    Tied(usize),
    /// No entry has a comparable value.
    None,
}

/// Pick the best (lowest) value among optional numbers.
pub fn best_metric(values: &[Option<f64>]) -> Best {
    let comparable: Vec<(usize, f64)> = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|k| (i, k)))
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

/// Keep only the most recent entry per (store, product) — by date, then id.
/// When comparing a product across stores, an older price in the same
/// store should not compete with the current one.
pub fn latest_per_store(entries: &[PriceEntry]) -> Vec<PriceEntry> {
    let mut latest: std::collections::HashMap<(u64, u64), &PriceEntry> =
        std::collections::HashMap::new();
    for e in entries {
        let key = (e.store_id, e.product_id);
        let replace = match latest.get(&key) {
            Some(cur) => (cur.date.as_str(), cur.id) < (e.date.as_str(), e.id),
            None => true,
        };
        if replace {
            latest.insert(key, e);
        }
    }
    let mut out: Vec<PriceEntry> = latest.into_values().cloned().collect();
    out.sort_by_key(|e| e.id);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn weighed(id: u64, price: &str, amount: &str, qty_unit: &str) -> PriceEntry {
        PriceEntry {
            qty_unit: qty_unit.into(),
            unit_weight: String::new(),
            ..entry(id, 1, price, amount, "", "")
        }
    }

    #[test]
    fn gram_quantity_gives_per_kg() {
        // 6.00 for 500 g of mince -> 12.00 /kg
        let q = entry_quote(&weighed(1, "6.00", "500", "g"));
        assert_eq!(q.per_unit, None);
        assert!((q.per_kg.unwrap() - 12.0).abs() < 1e-9);
        assert_eq!(q.per_l, None);
    }

    #[test]
    fn liter_quantity_gives_per_liter() {
        // 3.50 for 1.5 L of milk -> 2.33 /L
        let q = entry_quote(&weighed(1, "3.50", "1.5", "l"));
        assert_eq!(q.per_unit, None);
        assert_eq!(q.per_kg, None);
        assert!((q.per_l.unwrap() - 3.50 / 1.5).abs() < 1e-9);
    }

    #[test]
    fn simple_unit_price() {
        let q = entry_quote(&entry(1, 1, "0.50", "1", "", ""));
        assert_eq!(q.per_unit, Some(0.50));
        assert_eq!(q.per_kg, None);
    }

    #[test]
    fn multipack_divides_by_quantity() {
        // 4.99 for a 6-pack of bottles -> 0.83 each
        let q = entry_quote(&entry(1, 1, "4.99", "6", "", ""));
        assert!((q.per_unit.unwrap() - 4.99 / 6.0).abs() < 1e-9);
        assert_eq!(q.per_kg, None);
    }

    #[test]
    fn weight_gives_per_kg() {
        // 0.50 for 1 apple of 200 g -> 2.50 /kg
        let q = entry_quote(&entry(1, 1, "0.50", "1", "200", "g"));
        assert_eq!(q.per_unit, Some(0.50));
        assert!((q.per_kg.unwrap() - 2.5).abs() < 1e-9);
    }

    #[test]
    fn weight_multiplies_by_quantity() {
        // 4.99 for 6 bottles of 1.5 L(=kg)... use kg units: 6 x 1 kg
        let q = entry_quote(&entry(1, 1, "4.99", "6", "1", "kg"));
        assert!((q.per_kg.unwrap() - 4.99 / 6.0).abs() < 1e-9);
    }

    #[test]
    fn comma_and_currency_parse() {
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

    #[test]
    fn best_metric_picks_lowest() {
        assert_eq!(best_metric(&[Some(2.5), Some(2.99), Some(2.4)]), Best::Winner(2));
    }

    #[test]
    fn best_metric_ties() {
        assert_eq!(best_metric(&[Some(3.0), None, Some(3.0)]), Best::Tied(0));
    }

    #[test]
    fn best_metric_none() {
        assert_eq!(best_metric(&[None, None]), Best::None);
    }

    #[test]
    fn latest_per_store_keeps_newest() {
        let mut old = entry(1, 10, "2.99", "1", "", "");
        old.date = "2026-11-01".into();
        let mut new = entry(2, 10, "1.99", "1", "", "");
        new.promo = true;
        let other = entry(3, 20, "2.50", "1", "", "");
        let out = latest_per_store(&[old, new, other]);
        assert_eq!(out.len(), 2);
        assert!(out.iter().any(|e| e.id == 2)); // newer price in store 10
        assert!(out.iter().any(|e| e.id == 3));
    }

    #[test]
    fn latest_per_store_is_per_product() {
        let a = entry(1, 10, "2.99", "1", "", ""); // product 1
        let mut b = entry(2, 10, "3.50", "1", "", ""); // product 2, same store
        b.product_id = 2;
        let out = latest_per_store(&[a, b]);
        assert_eq!(out.len(), 2); // both kept — different products
    }

    #[test]
    fn categories_are_guessed_from_names() {
        assert_eq!(guess_category("Cucumber"), "vegetable");
        assert_eq!(guess_category("yellow kiwi"), "fruit");
        assert_eq!(guess_category("Bottle of water"), "bottle");
        assert_eq!(guess_category("Something weird"), "other");
    }
}
