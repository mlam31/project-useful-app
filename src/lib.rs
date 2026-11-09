// src/lib.rs

use wasm_bindgen::prelude::*;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum CheaperOption {
    PerUnit,
    PerKg,
    Equal,
}

pub struct PriceComparison {
    pub price_per_unit: f64,
    pub price_per_kg: f64,
    pub avg_weight_grams: f64,
}

pub struct ComparisonResult {
    pub equivalent_price_per_kg: f64,
    pub equivalent_price_per_unit: f64,
    pub cheaper: CheaperOption,
    pub savings_percentage: f64,
}

impl PriceComparison {
    pub fn new(price_per_unit: f64, price_per_kg: f64, avg_weight_grams: f64) -> Self {
        Self {
            price_per_unit,
            price_per_kg,
            avg_weight_grams,
        }
    }

    pub fn compare(&self) -> ComparisonResult {
        let weight_in_kg = self.avg_weight_grams / 1000.0;

        let equivalent_price_per_kg = if weight_in_kg > 0.0 {
            self.price_per_unit / weight_in_kg
        } else {
            0.0
        };

        let equivalent_price_per_unit = self.price_per_kg * weight_in_kg;

        let (cheaper, savings_percentage) = if (self.price_per_kg - equivalent_price_per_kg).abs() < 1e-5 {
            (CheaperOption::Equal, 0.0)
        } else if equivalent_price_per_kg < self.price_per_kg {
            let diff = self.price_per_kg - equivalent_price_per_kg;
            let savings = (diff / self.price_per_kg) * 100.0;
            (CheaperOption::PerUnit, savings)
        } else {
            let diff = equivalent_price_per_kg - self.price_per_kg;
            let savings = (diff / equivalent_price_per_kg) * 100.0;
            (CheaperOption::PerKg, savings)
        };

        ComparisonResult {
            equivalent_price_per_kg,
            equivalent_price_per_unit,
            cheaper,
            savings_percentage,
        }
    }
}

#[wasm_bindgen(start)]
pub fn run() {
    console_error_panic_hook::set_once();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_price_comparison() {
        let comp = PriceComparison::new(0.5, 4.0, 100.0);
        let res = comp.compare();
        assert_eq!(res.cheaper, CheaperOption::Equal);
    }
}
