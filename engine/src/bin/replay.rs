//! `replay`: read traces, apply every step and compare every state hash (E8).
//!
//! It prints one JSON line per valid file and stops with exit code 1 at the
//! first invalid one. Running it over the traces of the other implementation
//! is the cross-implementation check.
use catan_engine::trace;
use std::{
    fs::File,
    io::{self, BufRead, BufReader},
};
fn run() -> Result<(), String> {
    let paths: Vec<_> = std::env::args().skip(1).collect();
    if paths.iter().any(|p| p == "--help") {
        println!(
            "replay [TRACE.jsonl ...]\nNo paths or '-' reads stdin. Validates one complete constructor/snapshot trace per file."
        );
        return Ok(());
    }
    let paths = if paths.is_empty() {
        vec!["-".into()]
    } else {
        paths
    };
    for path in paths {
        let reader: Box<dyn BufRead> = if path == "-" {
            Box::new(io::stdin().lock())
        } else {
            Box::new(BufReader::new(
                File::open(&path).map_err(|e| format!("{path}: {e}"))?,
            ))
        };
        let r = trace::replay(reader).map_err(|e| format!("{path}: {e}"))?;
        println!(
            "{}",
            serde_json::json!({"file":path,"valid":true,"initial":if r.snapshot {"snapshot"} else {"constructor"},
            "steps":r.steps.to_string(),"decisions":r.decisions.to_string(),"chance_steps":r.chance_steps.to_string(),
            "turns":r.state.turn_number.to_string(),"winner":if r.state.winner<2 {Some(r.state.winner)}else{None},"hash":format!("{:016x}",r.state.hash())})
        );
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("replay: {e}");
        std::process::exit(1);
    }
}
