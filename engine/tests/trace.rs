//! E8: trace writing and strict replay, the dead result reason (E10), exact
//! chance sampling (E2), perft counts, and schedule-independent totals.
use catan_engine::{
    Board, Config, Outcome, RandomSource, SplitMix64, State,
    runner::{self, Choice},
    trace,
};
use serde_json::{Value, json};

fn prefix(snapshot: bool, steps: u64) -> Vec<u8> {
    let state = State::new(Board::default(), Config::default()).unwrap();
    let mut bytes = Vec::new();
    trace::generate(
        &mut bytes,
        state,
        &mut SplitMix64::new(41),
        snapshot,
        Some(steps),
    )
    .unwrap();
    bytes
}
fn records(bytes: &[u8]) -> Vec<Value> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}
fn encode(v: &[Value]) -> Vec<u8> {
    v.iter()
        .map(|v| format!("{v}\n"))
        .collect::<String>()
        .into_bytes()
}
fn dead_fixture() -> State {
    let bytes = trace::decode_hex(include_str!("fixtures/dead-seed191.hex").trim(), 313).unwrap();
    let state = State::from_canonical_bytes(&bytes).unwrap();
    assert!(state.is_dead());
    assert_eq!(state.cache.total_points, [14, 14]);
    assert_eq!(state.hash(), 0x4b3fd2661552e6c0);
    state
}

#[test]
fn e7_e8_constructor_and_snapshot_prefixes_roundtrip() {
    for snapshot in [false, true] {
        let bytes = prefix(snapshot, 20);
        let r = trace::replay(bytes.as_slice()).unwrap();
        assert_eq!(r.steps, 20);
        assert_eq!(r.snapshot, snapshot);
        assert_eq!(r.decisions + r.chance_steps, 20);
        assert!(r.chance_steps > 0);
        assert_eq!(
            State::from_canonical_bytes(&r.state.canonical_bytes())
                .unwrap()
                .hash(),
            r.state.hash()
        );
    }
}
#[test]
fn e8_replay_rejects_corrupt_structure_and_checksums() {
    let valid = records(&prefix(false, 1));
    let mut cases = Vec::new();
    let mut v = valid.clone();
    v[0]["encoding"] = json!(2);
    cases.push(v);
    let mut v = valid.clone();
    v[1].as_object_mut().unwrap().remove("dice");
    cases.push(v);
    let mut v = valid.clone();
    v[2]["hash"] = json!("FFFFFFFFFFFFFFFF");
    cases.push(v);
    let mut v = valid.clone();
    v[3]["index"] = json!("01");
    cases.push(v);
    let mut v = valid.clone();
    v[3]["index"] = json!(1);
    cases.push(v);
    let mut v = valid.clone();
    v[3]["index"] = json!("2");
    cases.push(v);
    let mut v = valid.clone();
    v[3]["outcome"] = json!(0);
    cases.push(v);
    let mut v = valid.clone();
    v[3]["action"] = json!(65536);
    cases.push(v);
    let mut v = valid.clone();
    v[3]["hash"] = json!("0000000000000000");
    cases.push(v);
    let mut v = valid.clone();
    v[4]["turns"] = json!("1");
    cases.push(v);
    let mut v = valid.clone();
    v[4]["status"] = json!("winner");
    v[4]["winner"] = json!(0);
    cases.push(v);
    let mut v = valid.clone();
    v[4].as_object_mut().unwrap().remove("winner");
    cases.push(v);
    let mut v = valid.clone();
    v.pop();
    cases.push(v);
    let mut v = valid.clone();
    v.push(valid[4].clone());
    cases.push(v);
    for (i, v) in cases.iter().enumerate() {
        assert!(trace::replay(encode(v).as_slice()).is_err(), "mutation {i}");
    }
    let mut blank = prefix(false, 0);
    blank.extend_from_slice(b"\n");
    assert!(trace::replay(blank.as_slice()).is_err());
}
#[test]
fn e8_snapshot_must_match_header_without_repair() {
    let mut v = records(&prefix(true, 0));
    v[1]["dice"] = json!(1);
    assert!(
        trace::replay(encode(&v).as_slice())
            .unwrap_err()
            .contains("board/config")
    );
    let mut v = records(&prefix(true, 0));
    let mut bytes = trace::decode_hex(v[2]["state_hex"].as_str().unwrap(), 313).unwrap();
    bytes[253] = 0;
    v[2]["state_hex"] = json!(trace::hex(&bytes));
    assert!(
        trace::replay(encode(&v).as_slice())
            .unwrap_err()
            .contains("Snapshot invalid")
    );
}
#[test]
fn e8_invalid_transition_reports_rule_context() {
    let mut v = records(&prefix(false, 1));
    v[3]["action"] = json!(0);
    let err = trace::replay(encode(&v).as_slice()).unwrap_err();
    assert!(
        err.contains("Line 4") && err.contains("S1-S4") && err.contains("choice=Some(Action(0))"),
        "{err}"
    );
}
#[test]
fn e8_winner_result_requires_terminal_and_exact_winner() {
    let state = State::new(
        Board::default(),
        Config {
            victory_points: 2,
            ..Config::default()
        },
    )
    .unwrap();
    let mut bytes = Vec::new();
    trace::generate(&mut bytes, state, &mut SplitMix64::new(3), false, None).unwrap();
    let report = trace::replay(bytes.as_slice()).unwrap();
    assert_eq!(report.steps, 8);
    assert_eq!(report.state.winner, 0);
    let mut v = records(&bytes);
    v.last_mut().unwrap()["winner"] = json!(1);
    assert!(trace::replay(encode(&v).as_slice()).is_err());
    let mut v = records(&bytes);
    v.last_mut().unwrap()["reason"] = json!("dead");
    assert!(trace::replay(encode(&v).as_slice()).is_err());
}
#[test]
fn e10_dead_result_roundtrips_and_rejects_false_reasons() {
    let state = dead_fixture();
    let mut bytes = Vec::new();
    let after = trace::generate(&mut bytes, state, &mut SplitMix64::new(3), true, None).unwrap();
    assert_eq!(after.canonical_bytes(), state.canonical_bytes());
    let report = trace::replay(bytes.as_slice()).unwrap();
    assert_eq!(report.steps, 0);
    assert!(report.snapshot);
    assert_eq!(report.state.hash(), state.hash());
    let valid = records(&bytes);
    assert_eq!(valid.len(), 4);
    assert_eq!(valid[3]["status"], "unfinished");
    assert!(valid[3]["winner"].is_null());
    assert_eq!(valid[3]["reason"], "dead");

    // Explicit prefixes can omit the reason, even at a dead state.
    let mut v = valid.clone();
    v[3].as_object_mut().unwrap().remove("reason");
    trace::replay(encode(&v).as_slice()).unwrap();
    for reason in [json!("draw"), json!("timeout"), Value::Null, json!(false)] {
        let mut v = valid.clone();
        v[3]["reason"] = reason;
        assert!(trace::replay(encode(&v).as_slice()).is_err());
    }
    let mut v = valid.clone();
    v[3]["status"] = json!("dead");
    assert!(trace::replay(encode(&v).as_slice()).is_err());
    let mut v = valid;
    v[3]["hash"] = json!("0000000000000000");
    assert!(trace::replay(encode(&v).as_slice()).is_err());

    let mut v = records(&prefix(false, 0));
    v[3]["reason"] = json!("dead");
    assert!(
        trace::replay(encode(&v).as_slice())
            .unwrap_err()
            .contains("requires an unfinished dead state")
    );
}

#[test]
fn e10_gen_cli_replays_the_closed_fixture_without_a_transition() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let state = dead_fixture();
    let mut child = Command::new(env!("CARGO_BIN_EXE_gen"))
        .args(["--snapshot", "/dev/stdin", "--seed", "191"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&state.canonical_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = trace::replay(output.stdout.as_slice()).unwrap();
    assert!(report.snapshot);
    assert_eq!(report.steps, 0);
    assert_eq!(report.state.canonical_bytes(), state.canonical_bytes());
    assert_eq!(records(&output.stdout)[3]["reason"], "dead");
}

#[test]
fn e10_choose_and_perft_preserve_dead_position_legal_transitions() {
    let state = dead_fixture();
    let choice = runner::choose(&state, &mut SplitMix64::new(9)).unwrap();
    let after = choice.apply(&state).unwrap();
    assert!(after.is_dead());
    assert_eq!(after.cache.total_points, state.cache.total_points);
    let mut actions = [0; 226];
    let n = state.legal_actions(&mut actions);
    assert!(n > 0);
    let perft = runner::perft(&state, 1).unwrap();
    assert_eq!(perft.leaves, n as u128);
    assert_eq!(perft.decision_edges, n as u128);
    assert_eq!(perft.chance_edges, 0);
    assert_eq!(perft.terminal_leaves, 0);
}
#[test]
fn e2_exact_sampling_uses_u128_and_rejects_biased_tail() {
    struct Script {
        values: std::vec::IntoIter<u64>,
    }
    impl RandomSource for Script {
        fn next_u64(&mut self) -> u64 {
            self.values.next().unwrap()
        }
    }
    // 2^128 mod 3 is 1, so the first zero draw must be rejected.
    let mut rng = Script {
        values: vec![0, 0, 0, 2].into_iter(),
    };
    let o = [
        Outcome {
            id: 11,
            weight: 1,
            total: 3,
        },
        Outcome {
            id: 12,
            weight: 2,
            total: 3,
        },
    ];
    assert_eq!(runner::sample_outcome(&o, &mut rng), 12);
    let mass = 1u128 << 100;
    let o = [
        Outcome {
            id: 0,
            weight: mass,
            total: mass * 2,
        },
        Outcome {
            id: 1,
            weight: mass,
            total: mass * 2,
        },
    ];
    let mut rng = Script {
        values: vec![1u64 << 36, 0].into_iter(),
    };
    assert_eq!(runner::sample_outcome(&o, &mut rng), 1);
}
#[test]
fn e8_parallel_seeds_and_totals_do_not_depend_on_schedule() {
    // Target two ends exactly at setup completion: a small positive control for
    // the scheduling rig, not a substitute for full default-policy measurements.
    let config = Config {
        victory_points: 2,
        ..Config::default()
    };
    let mut games: Vec<_> = (0..17)
        .map(|i| runner::prepare(9, i, config).unwrap())
        .collect();
    for seed in 0..3 {
        games.push(runner::PreparedGame {
            state: dead_fixture(),
            rng: SplitMix64::new(seed),
        });
    }
    let expected = runner::parallel(&games, 1).unwrap().0;
    for threads in 2..=16 {
        assert_eq!(runner::parallel(&games, threads).unwrap().0, expected);
    }
    assert_eq!(expected.attempts, 20);
    assert_eq!(expected.games, 20);
    assert_eq!(expected.winners, 17);
    assert_eq!(expected.dead_games, 3);
    assert_eq!(expected.winner_counts, [17, 0]);
    assert_eq!(expected.decisions, 17 * 8);
    assert_eq!(expected.chance_steps, 0);
    assert_eq!(expected.turns, 17 + 3 * dead_fixture().turn_number);
    assert!(runner::parallel(&games, 0).is_err());
}
#[test]
fn e2_perft_counts_explicit_decision_and_chance_edges() {
    let mut state = State::new(Board::default(), Config::default()).unwrap();
    assert_eq!(runner::perft(&state, 1).unwrap().leaves, 54);
    let mut rng = SplitMix64::new(7);
    for _ in 0..8 {
        state = runner::choose(&state, &mut rng)
            .unwrap()
            .apply(&state)
            .unwrap();
    }
    state = Choice::Action(0).apply(&state).unwrap();
    let p = runner::perft(&state, 1).unwrap();
    assert_eq!(p.chance_edges, 11);
    assert_eq!(p.decision_edges, 0);
    assert_eq!(p.leaves, 11);
}

#[test]
fn e8_replay_rejects_duplicate_required_fields() {
    let bytes = prefix(false, 0);
    let text = String::from_utf8(bytes).unwrap().replacen(
        "\"encoding\":1",
        "\"encoding\":0,\"encoding\":1",
        1,
    );
    assert!(
        trace::replay(text.as_bytes())
            .unwrap_err()
            .contains("Duplicate record field encoding")
    );
}
