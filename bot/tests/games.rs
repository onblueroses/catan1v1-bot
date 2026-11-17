//! Complete games through the arena: every event of `games.jsonl` and every result line.
mod common;

use catan_bot::arena::{PlayerKind, play_game};
use catan_bot::search::Settings;
use common::*;

fn run(kinds: &[&str]) {
    let nets = nets();
    let games: Vec<serde_json::Value> = read_jsonl("games.jsonl").into_iter()
        .filter(|g| kinds.contains(&g["kind"].as_str().unwrap())).collect();
    let fails = std::sync::Mutex::new(Vec::new());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let threads: usize = std::env::var("TEST_THREADS").ok().and_then(|x| x.parse().ok()).unwrap_or(16);
    std::thread::scope(|sc| {
        for _ in 0..threads {
            sc.spawn(|| loop {
                let j = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if j >= games.len() { break; }
                let g = &games[j];
                let kind = g["kind"].as_str().unwrap();
                let (a, b) = match kind {
                    "reply" => (PlayerKind::Bot(Settings { workers: 1, ..Settings::reply() }), PlayerKind::Bot(Settings { workers: 1, ..Settings::reply() })),
                    "mixed" => (PlayerKind::Bot(Settings { workers: 1, ..Settings::reply() }), PlayerKind::Random),
                    "random" => (PlayerKind::Random, PlayerKind::Random),
                    "vinco" => (PlayerKind::Bot(Settings { workers: 4, ..Settings::vinco() }), PlayerKind::Bot(Settings { workers: 4, ..Settings::vinco() })),
                    _ => panic!(),
                };
                let pair = g["pair"].as_u64().unwrap();
                let seat = g["seat_a"].as_u64().unwrap() as u8;
                let t0 = std::time::Instant::now();
                let r = play_game(&nets, &a, &b, pair, seat);
                let want_events: Vec<&str> = g["events"].as_str().unwrap().split_whitespace().collect();
                let first_diff = r.events.iter().zip(want_events.iter()).position(|(x, y)| x != y);
                let name = g["game"].as_str().unwrap();
                eprintln!("{name}: {} ({:.1} s)", r.line(), t0.elapsed().as_secs_f64());
                if r.board_pos.state_hex() != g["initial_state"].as_str().unwrap() {
                    fails.lock().unwrap().push(format!("{name}: initial state"));
                } else if first_diff.is_some() || r.events.len() != want_events.len() {
                    fails.lock().unwrap().push(format!("{name}: events differ at {:?} (len {} want {})", first_diff, r.events.len(), want_events.len()));
                } else if r.result_line() != g["result_line"].as_str().unwrap() {
                    fails.lock().unwrap().push(format!("{name}: result {} want {}", r.result_line(), g["result_line"]));
                }
            });
        }
    });
    let fails = fails.into_inner().unwrap();
    for f in &fails { eprintln!("{f}"); }
    eprintln!("games {kinds:?}: {} of {} match", games.len() - fails.len(), games.len());
    assert!(fails.is_empty());
}

#[test]
fn games_random_mixed_reply() { run(&["random", "mixed", "reply"]); }

#[test]
#[ignore]
fn games_vinco() { run(&["vinco"]); }
