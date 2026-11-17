//! Acceptance step 3: GCC sort, goal bonus, opening, beam, reply, belief, seeds,
//! rollout traces and full decisions.
mod common;

use catan_bot::belief::Belief;
use catan_bot::board::BoardInfo;
use catan_bot::decide::decide_traced;
use catan_bot::gccsort::gcc_sort;
use catan_bot::inputs::viewpoint;
use catan_bot::keys::{key181, root_seed};
use catan_bot::position::{Pos, hex_decode, hex_encode};
use catan_bot::rng::Rng;
use catan_bot::search::{Node, Search, Settings};
use catan_bot::value::{Mode, Valuer};
use common::*;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn gcc_sort_orders() {
    let text = std::fs::read_to_string(fixtures().join("sort-orders.txt")).unwrap();
    let mut n = 0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let (k, p) = line.split_once('|').unwrap();
        let keys: Vec<i64> = k.split_whitespace().map(|x| x.parse().unwrap()).collect();
        let perm: Vec<usize> = p.split_whitespace().map(|x| x.parse().unwrap()).collect();
        let mut v: Vec<(i64, usize)> = keys.iter().cloned().zip(0..).collect();
        gcc_sort(&mut v, |a, b| a.0 > b.0);
        let got: Vec<usize> = v.iter().map(|x| x.1).collect();
        assert_eq!(got, perm, "keys {keys:?}");
        n += 1;
    }
    eprintln!("sort orders: {n} match");
    assert_eq!(n, 275);
}

fn positions() -> HashMap<String, Value> {
    read_jsonl("positions.jsonl").into_iter().map(|p| (p["id"].as_str().unwrap().to_string(), p)).collect()
}

#[test]
fn opening_goal_rows() {
    let nets = nets();
    let pos = positions();
    let (mut n, mut bad) = (0, 0);
    for rec in read_jsonl("opening.jsonl") {
        let p = pos_of(&pos[rec["position"].as_str().unwrap()]);
        let v = rec["viewer"].as_u64().unwrap() as u8;
        let bi = BoardInfo::new(&p.st);
        let (vp, hid) = viewpoint(&p, v);
        let val = Valuer { nets: &nets, bi: &bi, mode: Mode::Opening, v, h: hid.h, n: hid.n, econ: 0.25, card: 0.1 };
        let s = Search::new(val, Settings::vinco(), hid.d);
        let root = Node { pos: vp, d: hid.d };
        for row in rec["rows"].as_array().unwrap() {
            let a = row[0].as_u64().unwrap() as u8;
            let c = root.child(a);
            let g0 = s.value(&c);
            let g1 = g0 + s.goal(&c, v, g0);
            let g2 = g1 - s.goal(&c, 1 - v, g1);
            n += 1;
            let got = [g0.to_bits(), g1.to_bits(), g2.to_bits()];
            let want = [bits(&row[1]), bits(&row[2]), bits(&row[3])];
            if got != want {
                bad += 1;
                if bad < 6 { eprintln!("opening {} a{a}: got {got:08x?} want {want:08x?}", rec["position"]); }
            }
        }
    }
    eprintln!("opening rows: {} of {n} match", n - bad);
    assert_eq!(bad, 0);
}

#[test]
fn belief_samples() {
    let (mut n, mut bad) = (0, 0);
    for rec in read_jsonl("retention.jsonl") {
        let pool: Vec<u8> = rec["pool"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u8).collect();
        let held = rec["held"].as_u64().unwrap() as u8;
        let cap = rec["cap"].as_i64().unwrap() as i32;
        let seed = u64::from_str_radix(rec["seed"].as_str().unwrap(), 16).unwrap();
        let b = Belief::new(pool.clone().try_into().unwrap(), held, cap);
        let defined = rec["defined"].as_bool().unwrap();
        let sample = b.as_ref().and_then(|b| b.sample(seed));
        let want: Option<Vec<u8>> = rec["sample"].as_array().map(|a| a.iter().map(|x| x.as_u64().unwrap() as u8).collect());
        n += 1;
        if b.is_some() != defined || sample.map(|s| s.to_vec()) != want {
            bad += 1;
            if bad < 6 { eprintln!("belief {rec}: got defined {} sample {sample:?}", b.is_some()); }
        }
    }
    eprintln!("belief: {} of {n} match", n - bad);
    assert_eq!(bad, 0);
}

fn settings_for(config: &str, workers: usize) -> Settings {
    let mut s = match config {
        "beam" => Settings::beam(),
        "reply" => Settings::reply(),
        "r16" => Settings { rollout_samples: 16, ..Settings::vinco() },
        "vinco" => Settings::vinco(),
        _ => panic!("config {config}"),
    };
    s.workers = workers;
    s
}

fn decision_pos(rec: &Value, pos: &HashMap<String, Value>) -> Pos {
    match rec.get("embedded") {
        Some(e) => pos_of(e),
        None => pos_of(&pos[rec["position"].as_str().unwrap()]),
    }
}

/// Check one decision record; returns a description of the first mismatch.
fn check_decision(rec: &Value, p: &Pos, nets: &catan_bot::net::Nets) -> Result<(), String> {
    let config = rec["config"].as_str().unwrap();
    let set = settings_for(config, 1);
    let seed: u64 = rec["rng_seed"].as_str().unwrap().parse().unwrap();
    let mut rng = Rng::seed(seed);
    let v = rec["viewer"].as_u64().unwrap() as u8;
    assert_eq!(v, p.mover);
    if let Some(k) = rec.get("key181") {
        let (vp, hid) = viewpoint(p, v);
        let key = key181(&vp, hid.d);
        if hex_encode(&key) != k.as_str().unwrap() {
            return Err(format!("key181 got {}", hex_encode(&key)));
        }
        let bi = BoardInfo::new(&p.st);
        let rs = root_seed(&key, v, hid, &bi, p.port_ring);
        let want = rec["sample_root_seed"].as_str().unwrap();
        let want = u64::from_str_radix(want, 16).unwrap();
        if rs != want {
            return Err(format!("root seed got {rs} want {want}"));
        }
    }
    let d = decide_traced(nets, &set, p, &mut rng, None);
    let want_vals: Vec<(u8, u32)> = rec["values"].as_array().unwrap().iter()
        .map(|x| (x[0].as_u64().unwrap() as u8, bits(&x[1]))).collect();
    let got_vals: Vec<(u8, u32)> = d.values.iter().map(|&(a, x)| (a, x.to_bits())).collect();
    let want_rng: Vec<u64> = rec["rng_state_after"].as_array().unwrap().iter()
        .map(|x| u64::from_str_radix(x.as_str().unwrap(), 16).unwrap()).collect();
    let mut errs = Vec::new();
    if d.action as u64 != rec["action"].as_u64().unwrap() {
        errs.push(format!("action {} want {}", d.action, rec["action"]));
    }
    if d.value.to_bits() != bits(&rec["value_bits"]) {
        errs.push(format!("value {:08x} want {}", d.value.to_bits(), rec["value_bits"]));
    }
    if got_vals != want_vals {
        let diff: Vec<String> = got_vals.iter().zip(want_vals.iter()).filter(|(a, b)| a != b).take(6)
            .map(|(a, b)| format!("{}:{:08x}/{}:{:08x}", a.0, a.1, b.0, b.1)).collect();
        errs.push(format!("values (got/want) n {} / {}: {}", got_vals.len(), want_vals.len(), diff.join(" ")));
    }
    if rng.s.to_vec() != want_rng {
        errs.push("rng state".to_string());
    }
    if errs.is_empty() { Ok(()) } else { Err(errs.join("; ")) }
}

/// Run the decision records whose config is in `configs` (env DECISIONS_LIMIT caps the count per config).
fn run_decisions(configs: &[&str], embedded: Option<bool>) {
    let nets = nets();
    let pos = positions();
    let limit: usize = std::env::var("DECISIONS_LIMIT").ok().and_then(|x| x.parse().ok()).unwrap_or(usize::MAX);
    let mut per: HashMap<String, usize> = HashMap::new();
    let recs: Vec<Value> = read_jsonl("decisions.jsonl").into_iter().filter(|r| {
        let c = r["config"].as_str().unwrap();
        if !configs.contains(&c) { return false; }
        if let Some(e) = embedded { if r.get("embedded").is_some() != e { return false; } }
        let n = per.entry(c.to_string()).or_default();
        *n += 1;
        *n <= limit
    }).collect();
    let next = AtomicUsize::new(0);
    let fails: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let threads: usize = std::env::var("TEST_THREADS").ok().and_then(|x| x.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4));
    std::thread::scope(|sc| {
        for _ in 0..threads {
            sc.spawn(|| loop {
                let j = next.fetch_add(1, Ordering::Relaxed);
                if j >= recs.len() { break; }
                let rec = &recs[j];
                let p = decision_pos(rec, &pos);
                if let Err(e) = check_decision(rec, &p, &nets) {
                    let id = rec["position"].as_str().unwrap();
                    fails.lock().unwrap().push(format!("{id} [{} ph{}]: {e}", rec["config"].as_str().unwrap(), p.phase));
                }
            });
        }
    });
    let fails = fails.into_inner().unwrap();
    for f in fails.iter().take(25) { eprintln!("{f}"); }
    eprintln!("decisions {configs:?} embedded {embedded:?}: {} of {} match", recs.len() - fails.len(), recs.len());
    assert!(fails.is_empty());
}

#[test]
fn decisions_beam() { run_decisions(&["beam"], None); }

#[test]
fn decisions_reply() { run_decisions(&["reply"], None); }

#[test]
#[ignore]
fn decisions_r16() { run_decisions(&["r16"], None); }

#[test]
#[ignore]
fn decisions_vinco_sampled() { run_decisions(&["vinco"], Some(false)); }

#[test]
#[ignore]
fn decisions_vinco_games() { run_decisions(&["vinco"], Some(true)); }

#[test]
fn rollout_traces() {
    let nets = nets();
    let pos = positions();
    for id in ["reply-p0-s0-e58", "reply-p1-s0-e295"] {
        let dir = fixtures().join("rollout-traces");
        let dec: Value = serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{id}.decision.json"))).unwrap()).unwrap();
        let want: Vec<Value> = std::fs::read_to_string(dir.join(format!("{id}.events.jsonl"))).unwrap()
            .lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        let p = pos_of(&pos[id]);
        let set = Settings { rollout_samples: 16, workers: 1, ..Settings::vinco() };
        let mut rng = Rng::seed(12345);
        let mut trace = Vec::new();
        let d = decide_traced(&nets, &set, &p, &mut rng, Some(&mut trace));
        let mut bad = 0;
        for (j, w) in want.iter().enumerate() {
            let Some(g) = trace.get(j) else { eprintln!("{id}: trace ends at {j} of {}", want.len()); bad += 1; break; };
            let f = |x: &Value| x.as_str().map(|s| s.parse::<f64>().unwrap());
            let ok = g.stage == w["stage"].as_str().unwrap()
                && g.seed.to_string() == w["seed"].as_str().unwrap()
                && g.detail == w["detail"].as_i64().unwrap()
                && hex_encode(&g.key177) == w["key177"].as_str().unwrap()
                && g.mean == f(&w["mean"]) && g.realized == f(&w["realized"])
                && g.correction == f(&w["correction"]) && g.raw == f(&w["raw"]) && g.value == f(&w["value"]);
            if !ok {
                bad += 1;
                eprintln!("{id} event {j}: got {:?} {} {:?} {:?} {:?}\n   want {}", g.stage, g.detail, g.mean, g.realized, g.correction, w);
                let wk = hex_decode(w["key177"].as_str().unwrap());
                let diffs: Vec<(usize, u8, u8)> = (0..177).filter(|&i| g.key177[i] != wk[i]).map(|i| (i, g.key177[i], wk[i])).collect();
                eprintln!("   key diffs {diffs:?}");
                break;
            }
        }
        if trace.len() != want.len() { eprintln!("{id}: {} events, want {}", trace.len(), want.len()); }
        assert_eq!(bad, 0);
        assert_eq!(trace.len(), want.len());
        assert_eq!(d.action as u64, dec["action"].as_u64().unwrap());
        assert_eq!(d.value.to_bits(), bits(&dec["value_bits"]));
        let want_vals: Vec<(u8, u32)> = dec["values"].as_array().unwrap().iter().map(|x| (x[0].as_u64().unwrap() as u8, bits(&x[1]))).collect();
        let got_vals: Vec<(u8, u32)> = d.values.iter().map(|&(a, x)| (a, x.to_bits())).collect();
        assert_eq!(got_vals, want_vals);
        eprintln!("{id}: {} events match", want.len());
    }
}
