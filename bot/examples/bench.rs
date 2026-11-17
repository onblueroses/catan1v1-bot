//! Time VINCO decisions on fixture positions: `bench [N] [CONFIG]` (one worker per decision).
use catan_bot::decide::decide;
use catan_bot::net::Nets;
use catan_bot::position::{Pos, Protocol};
use catan_bot::rng::Rng;
use catan_bot::search::Settings;
use std::collections::HashMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: usize = args.get(1).and_then(|x| x.parse().ok()).unwrap_or(20);
    let config = args.get(2).cloned().unwrap_or("vinco".into());
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let nets = Nets::load(&root.join("nets")).unwrap();
    let fx = root.join("tests/fixtures");
    let pos: HashMap<String, serde_json::Value> = std::fs::read_to_string(fx.join("positions.jsonl")).unwrap().lines()
        .map(|l| { let v: serde_json::Value = serde_json::from_str(l).unwrap(); (v["id"].as_str().unwrap().to_string(), v) }).collect();
    let mut set = Settings::preset(&config).unwrap_or(Settings { rollout_samples: 16, ..Settings::vinco() });
    set.workers = 1;
    let mut total = 0.0;
    let mut k = 0;
    for l in std::fs::read_to_string(fx.join("decisions.jsonl")).unwrap().lines() {
        let r: serde_json::Value = serde_json::from_str(l).unwrap();
        if r["config"] != "vinco" || r.get("embedded").is_some() { continue; }
        let p = &pos[r["position"].as_str().unwrap()];
        if p["protocol"]["phase"] != 7 { continue; }
        let pr: Protocol = serde_json::from_value(p["protocol"].clone()).unwrap();
        let ps = Pos::from_parts(p["state"].as_str().unwrap(), &pr).unwrap();
        let mut rng = Rng::seed(r["rng_seed"].as_str().unwrap().parse().unwrap());
        let t = std::time::Instant::now();
        let d = decide(&nets, &set, &ps, &mut rng);
        let dt = t.elapsed().as_secs_f64();
        total += dt;
        assert_eq!(d.action as u64, r["action"].as_u64().unwrap());
        k += 1;
        if k >= n { break; }
    }
    println!("{k} decisions, {:.3} s each (reference ms field is for 4 workers)", total / k as f64);
}
