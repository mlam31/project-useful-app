// src/lib.rs

pub struct PriceComparison {
    pub price_per_unit: f64,
    pub price_per_kg: f64,
    pub avg_weight_grams: f64, // e.g. 100g per kiwi
}

#[derive(Debug, PartialEq)]
pub enum CheaperOption {
    PerUnit,
    PerKg,
    Equal,
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
        // Convert avg weight in grams to kg
        let weight_in_kg = self.avg_weight_grams / 1000.0;

        // Calculate equivalent price per kg if bought by unit
        let equivalent_price_per_kg = if weight_in_kg > 0.0 {
            self.price_per_unit / weight_in_kg
        } else {
            0.0
        };

        // Calculate equivalent price per unit if bought by kg
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_price_comparison() {
        let comp = PriceComparison::new(0.5, 5.0, 100.0);
        let res = comp.compare();
        assert_eq!(res.cheaper, CheaperOption::Equal);
    }
}
