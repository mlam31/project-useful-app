// tests/integration_test.rs
use useful_app::PriceComparison;
use useful_app::CheaperOption;

#[test]
fn test_per_unit_cheaper() {
    let comparison = PriceComparison::new(0.5, 4.0, 100.0); // 0.5 $/unit, 5 $/kg, 100g per unit
    let result = comparison.compare();
    assert_eq!(result.cheaper, CheaperOption::PerKg);
    assert!((result.savings_percentage - 50.0).abs() < 1e-5);
}

#[test]
fn test_per_kg_cheaper() {
    let comparison = PriceComparison::new(2.0, 10.0, 50.0); // 2 $/unit, 10 $/kg, 50g per unit
    let result = comparison.compare();
    assert_eq!(result.cheaper, CheaperOption::PerKg);
    assert!((result.savings_percentage - 20.0).abs() < 1e-5);
}

#[test]
fn test_equal() {
    let comparison = PriceComparison::new(1.0, 20.0, 50.0); // 1 $/unit, 20 $/kg, 50g per unit
    let result = comparison.compare();
    assert_eq!(result.cheaper, CheaperOption::Equal);
    assert!((result.savings_percentage - 0.0).abs() < 1e-5);
}
