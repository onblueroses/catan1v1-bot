//! `perft`: count every path of decisions and chance outcomes to a fixed
//! depth. Two implementations that agree on these counts enumerate the same
//! legal actions and outcomes in every state down to that depth.
use catan_engine::{Config, State, runner};
fn run() -> Result<(), String> {
    let mut depth = None;
    let mut seed = 0u64;
    let mut config = Config::default();
    let mut snapshot = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" {
            println!(
                "perft --depth N [--seed N] [--dice balanced|independent] [--snapshot CANONICAL_FILE]\nDepth counts decision and chance transitions; leaves count paths, not probability mass. Dead positions retain every legal branch."
            );
            return Ok(());
        }
        let v = args
            .next()
            .ok_or_else(|| format!("Missing value for {arg}"))?;
        match arg.as_str() {
            "--depth" => depth = Some(v.parse().map_err(|_| "Invalid depth")?),
            "--seed" => seed = v.parse().map_err(|_| "Invalid seed")?,
            "--dice" => {
                config.dice = match v.as_str() {
                    "balanced" => 0,
                    "independent" => 1,
                    _ => return Err("Invalid dice".into()),
                }
            }
            "--snapshot" => snapshot = Some(v),
            _ => return Err(format!("Unknown option {arg}")),
        }
    }
    let depth = depth.ok_or("--depth is required (no implicit search budget)")?;
    let state = if let Some(path) = snapshot {
        State::from_canonical_bytes(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("{e:?}"))?
    } else {
        runner::prepare(seed, 0, config)
            .map_err(|e| format!("{e:?}"))?
            .state
    };
    let result = runner::perft(&state, depth)?;
    println!(
        "{}",
        serde_json::json!({"depth":depth,"leaves":result.leaves.to_string(),"decision_edges":result.decision_edges.to_string(),
        "chance_edges":result.chance_edges.to_string(),"terminal_leaves":result.terminal_leaves.to_string()})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("perft: {e}");
        std::process::exit(1);
    }
}
