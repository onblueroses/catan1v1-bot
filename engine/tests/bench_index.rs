//! Successful benchmark CLI controls belong to the remote correctness gate.
use catan_engine::{
    Config,
    runner::{self, Totals},
};
use serde_json::Value;
use std::process::Command;

fn invoke(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_bench"))
        .args(args)
        .output()
        .unwrap()
}

fn rows(args: &[&str]) -> Vec<Value> {
    let output = invoke(args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn serial(start: u64, count: u64, dice: u8) -> Totals {
    let mut total = Totals::default();
    for index in start..start + count {
        let game = runner::prepare(
            41,
            index,
            Config {
                dice,
                ..Config::default()
            },
        )
        .unwrap();
        total.combine(runner::play(game).unwrap());
    }
    total
}

fn check_result(row: &Value, expected: Totals) {
    for (field, value) in [
        ("attempts", expected.attempts),
        ("games", expected.games),
        ("winners", expected.winners),
        ("dead_games", expected.dead_games),
        ("decisions", expected.decisions),
        ("chance_steps", expected.chance_steps),
    ] {
        assert_eq!(row[field], value.to_string(), "{field}");
    }
    assert_eq!(
        row["winners_by_player"],
        serde_json::json!(expected.winner_counts)
    );
    assert_eq!(row["checksum"], format!("{:016x}", expected.checksum));
    assert_eq!(
        row["turns_per_game"],
        expected.turns as f64 / expected.games as f64
    );
    // Remeasured after combining geometry with the accepted road/compact core:
    // State 528/16, Cache 208/16, UndoRecord 88/8 (size/alignment).
    assert_eq!(row["state_bytes"], 528);
    assert_eq!(row["canonical_bytes"], 313);
}

#[test]
fn start_index_cli_rejects_overflow_and_invalid_values_before_playing() {
    for (games, start, message) in [
        (
            "2",
            "18446744073709551615",
            "Game index range overflows u64",
        ),
        (
            "3",
            "18446744073709551614",
            "Game index range overflows u64",
        ),
        ("1", "18446744073709551616", "Invalid start index"),
        ("1", "-1", "Invalid start index"),
    ] {
        let output = invoke(&["--games", games, "--start-index", start]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr).unwrap().contains(message));
    }
    let output = invoke(&["--games", "1", "--start-index"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Missing value for --start-index")
    );
}

#[test]
fn start_index_cli_defaults_to_zero_and_matches_indexed_serial_totals() {
    for (dice_name, dice) in [("balanced", 0), ("independent", 1)] {
        let base = [
            "--games",
            "2",
            "--seed",
            "41",
            "--threads",
            "2",
            "--dice",
            dice_name,
        ];
        let default = rows(&base).pop().unwrap();
        let explicit_args: Vec<_> = base
            .iter()
            .chain(["--start-index", "0"].iter())
            .copied()
            .collect();
        let explicit = rows(&explicit_args).pop().unwrap();
        assert_eq!(default["start_index"], "0");
        assert_eq!(explicit["start_index"], "0");
        let expected = serial(0, 2, dice);
        check_result(&default, expected);
        check_result(&explicit, expected);
        let held_out_args: Vec<_> = base
            .iter()
            .chain(["--start-index", "2048", "--worker-diagnostics"].iter())
            .copied()
            .collect();
        let held_out = rows(&held_out_args);
        assert_eq!(held_out.len(), 3);
        for row in &held_out {
            assert_eq!(row["start_index"], "2048");
            assert_eq!(row["scheduler"], "dynamic");
        }
        check_result(held_out.last().unwrap(), serial(2048, 2, dice));
    }
}
