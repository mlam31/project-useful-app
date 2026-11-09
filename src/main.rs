// src/main.rs
use std::env;
use useful_app::PriceComparison;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CLI arguments: price_per_unit price_per_kg avg_weight_grams
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("Usage: {} <price_per_unit> <price_per_kg> <avg_weight_grams>", args[0]);
        std::process::exit(1);
    }

    let price_per_unit = args[1].parse::<f64>()?;
    let price_per_kg = args[2].parse::<f64>()?;
    let avg_weight_grams = args[3].parse::<f64>()?;

    let comparison = PriceComparison::new(price_per_unit, price_per_kg, avg_weight_grams);
    let result = comparison.compare();

    println!("Equivalent price per kg if bought by unit: {:.2} $/kg", result.equivalent_price_per_kg);
    println!("Equivalent price per unit if bought by kg: {:.2} $/unit", result.equivalent_price_per_unit);
    println!("Cheaper option: {:?}", result.cheaper);
    println!("Savings percentage: {:.1}%", result.savings_percentage);

    Ok(())
}
