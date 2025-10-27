
// Simple CLI: read JSON input and output JSON result
use std::env;
use std::fs;
use slimechain_algo::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct CostInput {
    actor: Actor,
    content: Content,
    base_fare: Option<f64>,
}

#[derive(Serialize, Deserialize)]
struct PropInput {
    risk_signals: Option<RiskSignals>,
}

#[derive(Serialize, Deserialize)]
struct BaseInput {
    current_base: f64,
    current_load: f64,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: slimechain-algo <cost|reward|prop|base|quality|ef|risk> <input.json>");
        std::process::exit(1);
    }
    let cmd = &args[1];
    let path = &args[2];
    let data = fs::read_to_string(path).expect("Failed to read input file");
    let params = Params::default();

    match cmd.as_str() {
        "cost" => {
            let input: CostInput = serde_json::from_str(&data).expect("Failed to parse JSON");
            let base = input.base_fare.unwrap_or(1.0);
            let out = calculate_post_cost(&input.actor, &input.content, &params, base);
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "cost": out })).unwrap());
        },
        "reward" => {
            let input: RewardInput = serde_json::from_str(&data).expect("Failed to parse JSON");
            let out = calculate_serve_reward(&input, &params);
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "reward": out })).unwrap());
        },
        "prop" => {
            let input: PropInput = serde_json::from_str(&data).expect("Failed to parse JSON");
            let out = adjust_propagation(&input.risk_signals, &params);
            println!("{}", serde_json::to_string_pretty(&serde_json::to_value(out).unwrap()).unwrap());
        },
        "base" => {
            let input: BaseInput = serde_json::from_str(&data).expect("Failed to parse JSON");
            let out = update_base_cost(input.current_base, input.current_load, &params);
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "base": out })).unwrap());
        },
        "quality" => {
            let qin: QInputs = serde_json::from_str(&data).expect("Failed to parse JSON");
            let out = calculate_quality(qin, &params);
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "q": out })).unwrap());
        },
        "ef" => {
            let arr: Vec<f64> = serde_json::from_str(&data).expect("Failed to parse JSON (array required)");
            let out = calculate_ef(&arr, &params);
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "ef": out })).unwrap());
        },
        "risk" => {
            let sig: RiskSignals = serde_json::from_str(&data).expect("Failed to parse JSON");
            let out = calculate_risk(&Some(sig), &RiskWeights::default());
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "risk": out })).unwrap());
        },
        _ => {
            eprintln!("Unknown command: {}", cmd);
            std::process::exit(2);
        }
    }
}
