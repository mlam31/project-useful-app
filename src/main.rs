use leptos::*;
use useful_app::{PriceComparison, CheaperOption};

#[component]
pub fn App() -> impl IntoView {
    let (price_unit, set_price_unit) = create_signal(String::from("0.5"));
    let (price_kg, set_price_kg) = create_signal(String::from("4.0"));
    let (weight, set_weight) = create_signal(String::from("100"));

    let result = move || {
        let p_unit = price_unit.get().parse::<f64>().unwrap_or(0.0);
        let p_kg = price_kg.get().parse::<f64>().unwrap_or(0.0);
        let w_g = weight.get().parse::<f64>().unwrap_or(0.0);

        if p_unit > 0.0 && p_kg > 0.0 && w_g > 0.0 {
            let comp = PriceComparison::new(p_unit, p_kg, w_g);
            Some(comp.compare())
        } else {
            None
        }
    };

    view! {
        <div class="container">
            <h1>"Kiwi Price Comparator"</h1>

            <div class="form-group">
                <label>"Price per Unit ($)"</label>
                <input
                    type="number"
                    step="0.01"
                    value=price_unit
                    on:input=move |ev| set_price_unit.set(event_target_value(&ev))
                />
            </div>

            <div class="form-group">
                <label>"Price per Kg ($)"</label>
                <input
                    type="number"
                    step="0.01"
                    value=price_kg
                    on:input=move |ev| set_price_kg.set(event_target_value(&ev))
                />
            </div>

            <div class="form-group">
                <label>"Average Weight per Unit (g)"</label>
                <input
                    type="number"
                    step="1"
                    value=weight
                    on:input=move |ev| set_weight.set(event_target_value(&ev))
                />
            </div>

            {move || result().map(|res| {
                let cheaper_text = match res.cheaper {
                    CheaperOption::PerUnit => "Buying PER UNIT is cheaper!",
                    CheaperOption::PerKg => "Buying PER KG is cheaper!",
                    CheaperOption::Equal => "Both options cost the same!",
                };

                view! {
                    <div class="result">
                        <h3>{cheaper_text}</h3>
                        <p>{format!("Eq. Price per Kg: {:.2} $/kg", res.equivalent_price_per_kg)}</p>
                        <p>{format!("Eq. Price per Unit: {:.2} $/unit", res.equivalent_price_per_unit)}</p>
                        <p>{format!("Savings: {:.1}%", res.savings_percentage)}</p>
                    </div>
                }
            })}
        </div>
    }
}

pub fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(|| view! { <App/> })
}
