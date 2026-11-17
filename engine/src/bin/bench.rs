//! `bench`: throughput of complete random games, and per-operation costs (E8).
//!
//! The game benchmark plays `--games` prepared games under the uniform policy
//! on one or more threads and prints one JSON line per thread count. With
//! `--trace` and `--micro-repeats` it also times single operations (apply,
//! copy, legal-action generation, chance sampling) on the states of the given
//! traces. TOOLS.md describes every flag and what the timing includes.
use catan_engine::{
    Config, Outcome, RandomSource, SplitMix64, State,
    runner::{self, Choice, Scheduler},
    trace,
};
use serde_json::json;
use std::{fs::File, hint::black_box, io::BufReader, time::Instant};

/// Mean nanoseconds per item over `repetitions` passes of `work`, where one
/// pass handles `iterations` items.
fn ns_per(iterations: usize, repetitions: u64, mut work: impl FnMut()) -> f64 {
    let start = Instant::now();
    for _ in 0..repetitions {
        work();
    }
    start.elapsed().as_secs_f64() * 1e9 / (iterations as f64 * repetitions as f64)
}
/// Per-operation costs on a corpus of (state, recorded transition) pairs.
/// The `undo` figures here time a whole-state copy as the restore step. The
/// sparse make/unmake of `undo.rs` is measured by the `undo-bench` tool.
fn micro(corpus: &[(State, Choice)], repetitions: u64, start_index: u64) -> Result<(), String> {
    if corpus.is_empty() {
        return Err("Microbenchmark corpus has no normal-play transitions".into());
    }
    let decisions: Vec<_> = corpus
        .iter()
        .filter(|(_, c)| matches!(c, Choice::Action(_)))
        .collect();
    let chances: Vec<_> = corpus
        .iter()
        .filter(|(_, c)| matches!(c, Choice::Outcome(_)))
        .collect();
    let apply = ns_per(corpus.len(), repetitions, || {
        for (state, choice) in corpus {
            black_box(black_box(*choice).apply(black_box(state)).unwrap());
        }
    });
    let decision_apply = if decisions.is_empty() {
        None
    } else {
        Some(ns_per(decisions.len(), repetitions, || {
            for (state, choice) in &decisions {
                black_box(black_box(*choice).apply(black_box(state)).unwrap());
            }
        }))
    };
    let chance_apply = if chances.is_empty() {
        None
    } else {
        Some(ns_per(chances.len(), repetitions, || {
            for (state, choice) in &chances {
                black_box(black_box(*choice).apply(black_box(state)).unwrap());
            }
        }))
    };
    let copy = ns_per(corpus.len(), repetitions, || {
        for (state, _) in corpus {
            black_box(*black_box(state));
        }
    });
    // Snapshot restore is a true whole-State copy, with no claim of sparse undo.
    let undo = ns_per(corpus.len(), repetitions, || {
        for (state, _) in corpus {
            let saved = black_box(state);
            let mut current = black_box(*state);
            black_box(&mut current);
            current = *saved;
            black_box(current);
        }
    });
    let apply_undo = ns_per(corpus.len(), repetitions, || {
        for (state, choice) in corpus {
            let saved = black_box(*state);
            let mut current = black_box(*choice).apply(black_box(&saved)).unwrap();
            black_box(&mut current);
            current = black_box(saved);
            black_box(current);
        }
    });
    let legal = if decisions.is_empty() {
        None
    } else {
        Some(ns_per(decisions.len(), repetitions, || {
            let mut buffer = [0; 226];
            for (state, _) in &decisions {
                let n = black_box(state).legal_actions(black_box(&mut buffer));
                black_box(&buffer[..n]);
            }
        }))
    };
    let chancegen = if chances.is_empty() {
        None
    } else {
        Some(ns_per(chances.len(), repetitions, || {
            let mut buffer = [Outcome::default(); 11];
            for (state, _) in &chances {
                let n = black_box(state).chance_outcomes(black_box(&mut buffer));
                black_box(&buffer[..n]);
            }
        }))
    };
    // Pre-enumerate choices so this measures only policy selection and RNG.
    let action_lists: Vec<Vec<u16>> = decisions
        .iter()
        .map(|(s, _)| {
            let mut a = [0; 226];
            let n = s.legal_actions(&mut a);
            a[..n].to_vec()
        })
        .collect();
    let outcome_lists: Vec<Vec<Outcome>> = chances
        .iter()
        .map(|(s, _)| {
            let mut a = [Outcome::default(); 11];
            let n = s.chance_outcomes(&mut a);
            a[..n].to_vec()
        })
        .collect();
    let mut rng = SplitMix64::new(0);
    let policy = if action_lists.is_empty() {
        None
    } else {
        Some(ns_per(action_lists.len(), repetitions, || {
            for a in &action_lists {
                black_box(a[rng.below(black_box(a.len() as u64)) as usize]);
            }
        }))
    };
    let chance_sample = if outcome_lists.is_empty() {
        None
    } else {
        Some(ns_per(outcome_lists.len(), repetitions, || {
            for o in &outcome_lists {
                black_box(runner::sample_outcome(black_box(o), &mut rng));
            }
        }))
    };
    let mut phase_counts = [0usize; 14];
    for (s, _) in corpus {
        phase_counts[s.phase as usize] += 1;
    }
    println!(
        "{}",
        json!({"measurement":"micro","corpus":"all normal-play pre-transition states from supplied validated traces",
        "start_index":start_index.to_string(),
        "samples":corpus.len(),"decision_samples":decisions.len(),"chance_samples":chances.len(),"phase_counts":phase_counts,
        "repetitions":repetitions.to_string(),"state_bytes":std::mem::size_of::<State>(),"canonical_bytes":313,
        "apply_ns":apply,"decision_apply_ns":decision_apply,"chance_apply_ns":chance_apply,"copy_ns":copy,"snapshot_restore_ns":undo,"apply_snapshot_restore_ns":apply_undo,
        "legal_generation_ns":legal,"chance_generation_ns":chancegen,"uniform_policy_selection_ns":policy,"exact_chance_sampling_ns":chance_sample,
        "undo_method":"full_state_snapshot; sparse undo not implemented","compiler_barrier":"black_box inputs and outputs"})
    );
    Ok(())
}

fn prepare_games(
    seed: u64,
    start_index: u64,
    games: u64,
    config: Config,
) -> Result<Vec<runner::PreparedGame>, String> {
    // Reject an overflowing range before allocating or generating any boards.
    if games != 0 {
        start_index.checked_add(games - 1).ok_or_else(|| {
            format!("Game index range overflows u64: start index {start_index}, games {games}")
        })?;
    }
    (0..games)
        .map(|offset| {
            let index = start_index
                .checked_add(offset)
                .ok_or("Game index overflows u64")?;
            runner::prepare(seed, index, config).map_err(|e| format!("{e:?}"))
        })
        .collect()
}

fn run() -> Result<(), String> {
    let mut games = None;
    let mut seed = 0u64;
    let mut start_index = 0u64;
    let mut threads = 1usize;
    let mut scale = false;
    let mut scheduler = Scheduler::Dynamic;
    let mut worker_diagnostics = false;
    let mut config = Config::default();
    let mut traces = Vec::new();
    let mut repetitions = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" {
            println!(
                "bench --games N [--seed N] [--start-index N] [--threads N | --scale] [--scheduler dynamic|static] [--worker-diagnostics] [--dice balanced|independent] [--config JSON_FILE] [--micro-repeats N --trace TRACE ...]\n--games selects attempts. --start-index selects the first global game index (default 0); the last index must fit u64. Games/s counts reached win or dead results. The experimental default scheduler is dynamic. Static retains contiguous game partitions. --worker-diagnostics records indexed game results and prints worker durations/totals. Its recording overhead is timed, so use separate runs for uninstrumented throughput. --scale measures every thread count 1..16. Boards precede timing. No game length cap. Microbenchmarks require caller-selected traces and repetitions. Run headline measurements on a dedicated benchmark machine."
            );
            return Ok(());
        }
        if arg == "--scale" {
            scale = true;
            continue;
        }
        if arg == "--worker-diagnostics" {
            worker_diagnostics = true;
            continue;
        }
        let v = args
            .next()
            .ok_or_else(|| format!("Missing value for {arg}"))?;
        match arg.as_str() {
            "--games" => games = Some(v.parse::<u64>().map_err(|_| "Invalid games")?),
            "--seed" => seed = v.parse().map_err(|_| "Invalid seed")?,
            "--start-index" => start_index = v.parse().map_err(|_| "Invalid start index")?,
            "--threads" => threads = v.parse().map_err(|_| "Invalid threads")?,
            "--scheduler" => {
                scheduler = match v.as_str() {
                    "dynamic" => Scheduler::Dynamic,
                    "static" => Scheduler::Static,
                    _ => return Err("Invalid scheduler".into()),
                }
            }
            "--dice" => {
                config.dice = match v.as_str() {
                    "balanced" => 0,
                    "independent" => 1,
                    _ => return Err("Invalid dice".into()),
                }
            }
            "--config" => {
                config = serde_json::from_reader(File::open(v).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?
            }
            "--trace" => traces.push(v),
            "--micro-repeats" => {
                repetitions = Some(v.parse::<u64>().map_err(|_| "Invalid micro repeats")?)
            }
            _ => return Err(format!("Unknown option {arg}")),
        }
    }
    let games = games.ok_or("--games is required (caller specifies measured workload)")?;
    if games == 0 || threads == 0 || repetitions == Some(0) {
        return Err("games, threads and micro repetitions must be positive".into());
    }
    if traces.is_empty() != repetitions.is_none() {
        return Err("Supply both --trace and --micro-repeats".into());
    }
    let prepared = prepare_games(seed, start_index, games, config)?;
    let counts: Vec<_> = if scale {
        (1..=16).collect()
    } else {
        vec![threads]
    };
    let mut control = None;
    for threads in counts {
        let (t, elapsed) = if worker_diagnostics {
            let report = runner::parallel_diagnostics(&prepared, threads, scheduler)?;
            for worker in report.workers {
                println!(
                    "{}",
                    json!({"measurement":"worker_diagnostic","scheduler":scheduler.name(),
                    "start_index":start_index.to_string(),
                    "threads":threads,"worker":worker.worker,"seconds":worker.elapsed.as_secs_f64(),
                    "attempts":worker.totals.attempts.to_string(),"decisions":worker.totals.decisions.to_string(),
                    "chance_steps":worker.totals.chance_steps.to_string(),"games":worker.totals.games.to_string(),
                    "winners":worker.totals.winners.to_string(),"dead_games":worker.totals.dead_games.to_string(),
                    "checksum":format!("{:016x}",worker.totals.checksum),"diagnostic_only":true})
                );
            }
            (report.totals, report.elapsed)
        } else {
            match scheduler {
                Scheduler::Static => runner::parallel_static(&prepared, threads)?,
                Scheduler::Dynamic => runner::parallel(&prepared, threads)?,
            }
        };
        if t.attempts != games
            || t.games != t.attempts
            || t.winners + t.dead_games != t.games
            || t.winner_counts.iter().sum::<u64>() != t.winners
        {
            return Err("Reached win/dead results do not conserve attempts".into());
        }
        if let Some(expected) = control {
            if t != expected {
                return Err("Thread counts changed deterministic game totals".into());
            }
        } else {
            control = Some(t);
        }
        let secs = elapsed.as_secs_f64();
        println!(
            "{}",
            json!({"measurement":"games","threads":threads,"scheduler":scheduler.name(),"worker_diagnostics":worker_diagnostics,
            "start_index":start_index.to_string(),
            "attempts":t.attempts.to_string(),"games":t.games.to_string(),
            "winners":t.winners.to_string(),"dead_games":t.dead_games.to_string(),"seed":seed.to_string(),"dice":config.dice,
            "policy":runner::POLICY,"seconds":secs,"games_per_second":t.games as f64/secs,"decisions_per_second":t.decisions as f64/secs,
            "attempts_per_second":t.attempts as f64/secs,"won_games_per_second":t.winners as f64/secs,
            "decisions":t.decisions.to_string(),"chance_steps":t.chance_steps.to_string(),"decisions_per_game":t.decisions as f64/t.games as f64,
            "turns_per_game":t.turns as f64/t.games as f64,"winner_fraction":t.winners as f64/t.games as f64,"winners_by_player":t.winner_counts,
            "dead_fraction":t.dead_games as f64/t.games as f64,"game_result":"winner_or_dead",
            "state_bytes":std::mem::size_of::<State>(),"canonical_bytes":313,"checksum":format!("{:016x}",t.checksum),
            "includes":if worker_diagnostics {"transitions, legal/chance enumeration, dead checks, policy RNG, final hash, thread launch/join, indexed game diagnostic recording"} else {"transitions, legal/chance enumeration, dead checks, policy RNG, final hash, thread launch/join"},"excludes":"board generation, constructor, JSON"})
        );
    }
    if let Some(repetitions) = repetitions {
        let mut corpus = Vec::new();
        for path in traces {
            trace::replay_with(
                BufReader::new(File::open(&path).map_err(|e| format!("{path}: {e}"))?),
                |s, c| {
                    if s.turn_number > 0 {
                        corpus.push((*s, c));
                    }
                },
            )
            .map_err(|e| format!("{path}: {e}"))?;
        }
        micro(&corpus, repetitions, start_index)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_preparation_matches_existing_indexed_inputs() {
        for dice in [0, 1] {
            let config = Config {
                dice,
                ..Config::default()
            };
            // Index zero is the original default. 2048 starts the held-out half
            // of the existing seed-41, 4096-game validation cohort.
            for start in [0, 2048] {
                let games = prepare_games(41, start, 2, config).unwrap();
                for (offset, game) in games.iter().enumerate() {
                    let expected = runner::prepare(41, start + offset as u64, config).unwrap();
                    assert_eq!(game.state, expected.state);
                    assert_eq!(
                        game.state.canonical_bytes(),
                        expected.state.canonical_bytes()
                    );
                    assert_eq!(game.rng.state, expected.rng.state);
                }
            }
        }
    }

    #[test]
    fn indexed_preparation_checks_the_inclusive_last_index() {
        let config = Config::default();
        assert!(
            prepare_games(41, u64::MAX, 2, config)
                .err()
                .unwrap()
                .contains("overflows u64")
        );
        assert!(
            prepare_games(41, u64::MAX - 1, 3, config)
                .err()
                .unwrap()
                .contains("overflows u64")
        );
        let games = prepare_games(41, u64::MAX, 1, config).unwrap();
        let expected = runner::prepare(41, u64::MAX, config).unwrap();
        assert_eq!(games[0].state, expected.state);
        assert_eq!(games[0].rng.state, expected.rng.state);
    }
}
fn main() {
    if let Err(e) = run() {
        eprintln!("bench: {e}");
        std::process::exit(1);
    }
}
