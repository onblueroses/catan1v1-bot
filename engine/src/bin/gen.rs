//! `gen`: play random games and write one JSONL trace per game (E8).
//!
//! A game runs to a winner or to a dead position (E10). `--prefix-steps`
//! stops earlier on request. With `--out DIR` each game gets its own file,
//! and an existing file is never overwritten. TOOLS.md describes every flag.
use catan_engine::{Config, SplitMix64, State, runner, trace};
use std::{
    fs::{File, OpenOptions},
    io::{self, BufWriter, Read, Write},
    path::PathBuf,
};

fn run() -> Result<(), String> {
    let mut seed = 0u64;
    let mut games = 1u64;
    let mut prefix = None;
    let mut out: Option<PathBuf> = None;
    let mut config = Config::default();
    let mut snapshot = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" {
            println!(
                "gen [--seed N] [--games N] [--dice balanced|independent] [--out DIR] [--prefix-steps N] [--snapshot CANONICAL_FILE] [--config JSON_FILE]\nOne game per JSONL file. Default: play to a winner or dead position and write to stdout. Dead results are unfinished with reason dead. Existing files are never overwritten."
            );
            return Ok(());
        }
        let value = args
            .next()
            .ok_or_else(|| format!("Missing value for {arg}"))?;
        match arg.as_str() {
            "--seed" => seed = value.parse().map_err(|_| "Invalid seed")?,
            "--games" => games = value.parse().map_err(|_| "Invalid games")?,
            "--prefix-steps" => prefix = Some(value.parse().map_err(|_| "Invalid prefix steps")?),
            "--out" => out = Some(value.into()),
            "--snapshot" => snapshot = Some(value),
            "--config" => {
                config = serde_json::from_reader(File::open(value).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?
            }
            "--dice" => {
                config.dice = match value.as_str() {
                    "balanced" => 0,
                    "independent" => 1,
                    _ => return Err("dice must be balanced or independent".into()),
                }
            }
            _ => return Err(format!("Unknown option {arg}")),
        }
    }
    if games == 0 || (games != 1 && out.is_none()) {
        return Err("Positive --games required; multiple games require --out DIR".into());
    }
    let snapshot_state = if let Some(path) = snapshot {
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(|e| e.to_string())?
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        Some(State::from_canonical_bytes(&bytes).map_err(|e| format!("Snapshot: {e:?}"))?)
    } else {
        None
    };
    if let Some(dir) = &out {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    for index in 0..games {
        let (state, mut rng) = if let Some(s) = snapshot_state {
            (s, SplitMix64::new(runner::game_seed(seed, index)))
        } else {
            let prepared = runner::prepare(seed, index, config).map_err(|e| format!("{e:?}"))?;
            (prepared.state, prepared.rng)
        };
        let mut writer: Box<dyn Write> = if let Some(dir) = &out {
            let path = dir.join(format!(
                "rust-dice{}-seed{seed}-game{index}.jsonl",
                state.config.dice
            ));
            Box::new(BufWriter::new(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|e| format!("{}: {e}", path.display()))?,
            ))
        } else {
            Box::new(BufWriter::new(io::stdout().lock()))
        };
        trace::generate(
            &mut writer,
            state,
            &mut rng,
            snapshot_state.is_some(),
            prefix,
        )?;
        writer.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("gen: {e}");
        std::process::exit(1);
    }
}
