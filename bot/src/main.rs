//! The `bot` command-line interface (BOT-SPEC 10).

use catan_bot::arena::{PlayerKind, play_game};
use catan_bot::board::BoardInfo;
use catan_bot::decide::decide;
use catan_bot::inputs::{compute_inputs, viewpoint};
use catan_bot::net::{Net, Nets};
use catan_bot::position::{Pos, Protocol, engine_sequence, hex_decode, hex_encode};
use catan_bot::rng::Rng;
use catan_bot::search::Settings;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage:
  bot decide [--preset vinco|reply|beam] [--rollouts N] [--workers N] [--seed S] [--nets DIR]
  bot inputs
  bot value --net FILE [--float|--fixed]
  bot arena A B FIRST_PAIR PAIRS [--workers N] [--nets DIR]
A and B: vinco, reply, beam, random or cmd:PROGRAM";

struct Opts {
    flags: Vec<(String, Option<String>)>,
    rest: Vec<String>,
}

fn parse_opts(args: &[String], valued: &[&str]) -> Result<Opts, String> {
    let mut flags = Vec::new();
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(name) = a.strip_prefix("--") {
            if valued.contains(&name) {
                let v = args.get(i + 1).ok_or(format!("--{name} needs a value"))?;
                flags.push((name.to_string(), Some(v.clone())));
                i += 2;
                continue;
            }
            flags.push((name.to_string(), None));
        } else {
            rest.push(a.clone());
        }
        i += 1;
    }
    Ok(Opts { flags, rest })
}

impl Opts {
    fn get(&self, name: &str) -> Option<&str> {
        self.flags.iter().rev().find(|(n, _)| n == name).and_then(|(_, v)| v.as_deref())
    }
    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(n, _)| n == name)
    }
}

fn parse_pos(v: &Value) -> Result<Pos, String> {
    let state = v["state"].as_str().ok_or("missing state")?;
    let pr: Protocol = serde_json::from_value(v["protocol"].clone()).map_err(|e| format!("protocol: {e}"))?;
    Pos::from_parts(state, &pr)
}

fn f32_json(x: f32) -> Value {
    serde_json::Number::from_f64(x as f64).map(Value::Number).unwrap_or(Value::Null)
}

fn cmd_decide(args: &[String]) -> Result<(), String> {
    let o = parse_opts(args, &["preset", "rollouts", "workers", "seed", "nets"])?;
    let mut set = Settings::preset(o.get("preset").unwrap_or("vinco")).ok_or("unknown preset")?;
    if let Some(r) = o.get("rollouts") {
        set.rollout_samples = r.parse().map_err(|_| "bad --rollouts")?;
    }
    set.workers = o.get("workers").map(|w| w.parse()).transpose().map_err(|_| "bad --workers")?.unwrap_or(4);
    let default_seed: u64 = o.get("seed").map(|s| s.parse()).transpose().map_err(|_| "bad --seed")?.unwrap_or(0);
    let nets = Nets::load(&PathBuf::from(o.get("nets").unwrap_or("nets")))?;
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(&line).map_err(|e| format!("json: {e}"))?;
        // Accept a bare position or the arena's {"position", "rng_seed"} message.
        let (pv, seed) = match v.get("position") {
            Some(p) => (p.clone(), v["rng_seed"].as_str().and_then(|s| s.parse().ok()).unwrap_or(default_seed)),
            None => (v.clone(), default_seed),
        };
        let pos = parse_pos(&pv)?;
        if pos.done() || pos.is_chance() {
            return Err("position is not a decision".into());
        }
        let mut rng = Rng::seed(seed);
        let d = decide(&nets, &set, &pos, &mut rng);
        let engine: Vec<u16> = if d.action == 181 && pos.phase == 9 {
            Vec::new()
        } else {
            let (seq, n) = engine_sequence(d.action);
            seq[..n].to_vec()
        };
        let r = json!({
            "action": d.action,
            "engine_actions": engine,
            "value": f32_json(d.value),
            "values": d.values.iter().map(|&(a, x)| json!([a, f32_json(x)])).collect::<Vec<_>>(),
            "rng_state_after": rng.s.iter().map(|w| format!("{w:016x}")).collect::<Vec<_>>(),
        });
        writeln!(out, "{r}").map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn cmd_inputs() -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(&line).map_err(|e| format!("json: {e}"))?;
        let pos = parse_pos(v.get("position").unwrap_or(&v))?;
        let bi = BoardInfo::new(&pos.st);
        let mut hexes = Vec::new();
        for viewer in 0..2u8 {
            let (vp, hid) = viewpoint(&pos, viewer);
            let mut x = [0u8; 351];
            compute_inputs(&vp, viewer, hid, &bi, &mut x);
            hexes.push(hex_encode(&x));
        }
        writeln!(out, "{}", json!({"inputs": hexes})).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn cmd_value(args: &[String]) -> Result<(), String> {
    let o = parse_opts(args, &["net"])?;
    let net = Net::load(&PathBuf::from(o.get("net").ok_or("--net FILE is required")?))?;
    let fixed = o.has("fixed");
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let b = hex_decode(t);
        if b.len() < net.w0 {
            return Err(format!("need {} input bytes, got {}", net.w0, b.len()));
        }
        let mut x = [0u8; 351];
        x[..b.len().min(351)].copy_from_slice(&b[..b.len().min(351)]);
        let y = if fixed { net.eval_fixed(&x) } else { net.eval_float(&x) };
        writeln!(out, "{:08x}", y.to_bits()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn cmd_arena(args: &[String]) -> Result<(), String> {
    let o = parse_opts(args, &["workers", "nets"])?;
    if o.rest.len() != 4 {
        return Err(USAGE.into());
    }
    let workers = o.get("workers").map(|w| w.parse()).transpose().map_err(|_| "bad --workers")?.unwrap_or(4);
    let a = PlayerKind::parse(&o.rest[0], workers).ok_or("bad player A")?;
    let b = PlayerKind::parse(&o.rest[1], workers).ok_or("bad player B")?;
    let first: u64 = o.rest[2].parse().map_err(|_| "bad FIRST_PAIR")?;
    let pairs: u64 = o.rest[3].parse().map_err(|_| "bad PAIRS")?;
    let nets = Nets::load(&PathBuf::from(o.get("nets").unwrap_or("nets")))?;
    let mut out = std::io::stdout().lock();
    for p in first..first + pairs {
        for seat in 0..2u8 {
            let g = play_game(&nets, &a, &b, p, seat);
            writeln!(out, "{}", g.line()).map_err(|e| e.to_string())?;
            out.flush().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("decide") => cmd_decide(&args[1..]),
        Some("inputs") => cmd_inputs(),
        Some("value") => cmd_value(&args[1..]),
        Some("arena") => cmd_arena(&args[1..]),
        _ => Err(USAGE.into()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
