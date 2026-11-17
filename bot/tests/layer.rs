//! Acceptance step 1-2 and test plan 2-5: decision layer, inputs, networks, search values.
mod common;

use catan_bot::board::BoardInfo;
use catan_bot::inputs::{Hidden, compute_inputs, viewpoint};
use catan_bot::position::{Pos, hex_decode};
use catan_bot::value::{Mode, Valuer};
use catan_engine::State;
use common::*;
use std::collections::HashMap;

#[test]
fn decision_layer_replays_games() {
    let positions = read_jsonl("positions.jsonl");
    let mut by_game: HashMap<String, Vec<&serde_json::Value>> = HashMap::new();
    for p in &positions {
        by_game.entry(p["game"].as_str().unwrap().to_string()).or_default().push(p);
    }
    let games = read_jsonl("games.jsonl");
    let mut checked = 0;
    let mut bad = 0;
    for g in &games {
        let st = State::from_compatibility_bytes(&hex_decode(g["initial_state"].as_str().unwrap())).unwrap();
        let ring = g["board"]["port_ring"].as_u64().unwrap() as u8;
        let mut pos = Pos::initial(st, ring, 10000);
        let name = g["game"].as_str().unwrap();
        let mut want: HashMap<u64, &serde_json::Value> = HashMap::new();
        for p in by_game.get(name).map(|v| v.as_slice()).unwrap_or(&[]) {
            want.insert(p["event"].as_u64().unwrap(), p);
        }
        let events: Vec<&str> = g["events"].as_str().unwrap().split_whitespace().collect();
        for k in 0..=events.len() {
            if let Some(p) = want.get(&(k as u64)) {
                checked += 1;
                let exp = pos_of(p);
                let mut ok = exp == pos;
                if let Some(legal) = p.get("legal") {
                    let l: Vec<u8> = legal.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u8).collect();
                    ok &= pos.legal().as_slice() == l.as_slice();
                }
                if let Some(ch) = p.get("chances") {
                    let (o, n) = pos.outcomes();
                    let got: Vec<(u64, u64)> = o[..n].iter().map(|&(i, w)| (i as u64, w as u64)).collect();
                    let exp: Vec<(u64, u64)> = ch.as_array().unwrap().iter()
                        .map(|x| (x[0].as_u64().unwrap(), x[1].as_u64().unwrap())).collect();
                    ok &= got == exp;
                }
                if !ok {
                    bad += 1;
                    if bad < 5 {
                        eprintln!("mismatch at {}: got {:?} want {:?}", p["id"], pos.protocol(), exp.protocol());
                        eprintln!("  state equal: {}", pos.st == exp.st);
                    }
                }
            }
            if k == events.len() { break; }
            let e = events[k];
            let x: u8 = e[1..].parse().unwrap();
            pos = if e.starts_with('a') {
                assert!(pos.legal().as_slice().contains(&x), "{name} event {k}: illegal {x}");
                pos.apply(x)
            } else {
                pos.apply_outcome(x)
            };
        }
        assert!(pos.done(), "{name} not done");
        let winner = match pos.winner() {
            None => "-".to_string(),
            Some(w) => {
                let seat_a = g["seat_a"].as_u64().unwrap() as u8;
                if w == seat_a { "A".into() } else { "B".into() }
            }
        };
        assert_eq!(winner, g["winner"].as_str().unwrap(), "{name} winner");
        assert_eq!(pos.st.turn_number - 1, g["turns"].as_u64().unwrap(), "{name} turns");
        assert_eq!(pos.step as u64, g["steps"].as_u64().unwrap(), "{name} steps");
    }
    eprintln!("decision layer: {} of {checked} positions match", checked - bad);
    assert_eq!(bad, 0);
    assert_eq!(checked, positions.len());
}

#[test]
fn inputs_nets_and_values() {
    let nets = nets();
    assert_eq!(nets.main.bound, 32976.066788867116);
    assert_eq!(nets.main.scale, 16384.0);
    assert_eq!(nets.main.unit.to_bits(), 0x3523d70a);
    assert_eq!(nets.opening.bound, 32111.709552168846);
    assert_eq!(nets.opening.scale, 32768.0);
    assert_eq!(nets.opening.unit.to_bits(), 0x34a3d70a);
    let positions = read_jsonl("positions.jsonl");
    let (mut n_in, mut bad_in) = (0, 0);
    let (mut n_net, mut bad_net) = (0, 0);
    let (mut n_sv, mut bad_sv) = (0, 0);
    let mut first_bad_inputs = 0;
    for p in &positions {
        let pos = pos_of(p);
        let bi = BoardInfo::new(&pos.st);
        for view in p["views"].as_array().unwrap() {
            let v = view["viewer"].as_u64().unwrap() as u8;
            let (vp, hid) = viewpoint(&pos, v);
            assert_eq!(hid, Hidden {
                h: view["rival_held"].as_u64().unwrap() as u8,
                n: view["rival_new"].as_u64().unwrap() as u8,
                d: view["deck_left"].as_u64().unwrap() as u8,
            });
            let mut x = [0u8; 351];
            compute_inputs(&vp, v, hid, &bi, &mut x);
            let want = hex_decode(view["inputs"].as_str().unwrap());
            n_in += 1;
            if x[..] != want[..] {
                bad_in += 1;
                if first_bad_inputs < 8 {
                    first_bad_inputs += 1;
                    let diffs: Vec<(usize, u8, u8)> = (0..351).filter(|&i| x[i] != want[i]).map(|i| (i, x[i], want[i])).collect();
                    eprintln!("inputs {} v{v}: (idx, got, want) {:?}", p["id"], &diffs[..diffs.len().min(12)]);
                }
                continue;
            }
            for (key, val) in [
                (&view["a320c1l"]["float"], nets.main.eval_float(&x)),
                (&view["a320c1l"]["fixed"], nets.main.eval_fixed(&x)),
                (&view["s320b"]["float"], nets.opening.eval_float(&x)),
                (&view["s320b"]["fixed"], nets.opening.eval_fixed(&x)),
            ] {
                n_net += 1;
                if val.to_bits() != bits(key) {
                    bad_net += 1;
                    if bad_net < 5 { eprintln!("net {} v{v}: {:08x} want {}", p["id"], val.to_bits(), key); }
                }
            }
            let mode = if pos.phase <= 1 { Mode::Opening } else { Mode::Main };
            let mut val = Valuer { nets: &nets, bi: &bi, mode, v, h: hid.h, n: hid.n, econ: 0.25, card: 0.1 };
            n_sv += 1;
            let sv = val.value(&vp, hid.d);
            if sv.to_bits() != bits(&view["search_value"]) {
                bad_sv += 1;
                if bad_sv < 5 { eprintln!("search value {} v{v}: {:08x} want {}", p["id"], sv.to_bits(), view["search_value"]); }
            }
            if let Some(rp) = view.get("rollout_player_value") {
                val.econ = 0.0;
                n_sv += 1;
                let sv = val.value(&vp, hid.d);
                if sv.to_bits() != bits(rp) {
                    bad_sv += 1;
                    if bad_sv < 5 { eprintln!("rollout value {} v{v}: {:08x} want {}", p["id"], sv.to_bits(), rp); }
                }
            }
        }
    }
    eprintln!("inputs: {} of {n_in} match", n_in - bad_in);
    eprintln!("net values: {} of {n_net} match", n_net - bad_net);
    eprintln!("search values: {} of {n_sv} match", n_sv - bad_sv);
    assert_eq!((bad_in, bad_net, bad_sv), (0, 0, 0));
}
