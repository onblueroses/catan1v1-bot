//! The parallel runner: static and dynamic scheduling must give the totals of
//! a serial run, game by game, for any thread count, including more threads
//! than games and no games at all.
use catan_engine::{
    Config, RandomSource, SplitMix64, State,
    runner::{self, ParallelDiagnostics, PreparedGame, Scheduler, Totals},
    trace,
};

fn serial(games: &[PreparedGame]) -> (Totals, Vec<Totals>) {
    let results: Vec<_> = games
        .iter()
        .map(|&game| runner::play(game).unwrap())
        .collect();
    let mut total = Totals::default();
    for &result in &results {
        total.combine(result);
    }
    (total, results)
}

fn check_records(report: ParallelDiagnostics, expected: &[Totals], threads: usize) {
    assert_eq!(report.workers.len(), threads);
    let mut visits = vec![0; expected.len()];
    let mut total = Totals::default();
    for (worker_index, worker) in report.workers.into_iter().enumerate() {
        assert_eq!(worker.worker, worker_index);
        let mut local = Totals::default();
        for game in worker.games {
            visits[game.index] += 1;
            assert_eq!(game.totals, expected[game.index], "game {}", game.index);
            local.combine(game.totals);
        }
        assert_eq!(worker.totals, local);
        total.combine(local);
    }
    assert_eq!(visits, vec![1; expected.len()]);
    assert_eq!(report.totals, total);
}

#[test]
fn dynamic_and_static_match_each_serial_game_for_both_dice() {
    for dice in [0, 1] {
        let config = Config {
            dice,
            ..Config::default()
        };
        // Three games and two workers also exercise an uneven static partition.
        let games: Vec<_> = (0..3)
            .map(|index| {
                let mut game = runner::prepare(41, index, config).unwrap();
                // Retain a supplied RNG position rather than reconstructing it
                // from a seed, including extra draws after board generation.
                for _ in 0..=index {
                    game.rng.next_u64();
                }
                game
            })
            .collect();
        let inputs: Vec<_> = games
            .iter()
            .map(|game| (game.state.canonical_bytes(), game.rng.state))
            .collect();
        let (expected_total, expected_games) = serial(&games);
        assert_eq!(
            runner::parallel_static(&games, 2).unwrap().0,
            expected_total
        );
        assert_eq!(runner::parallel(&games, 2).unwrap().0, expected_total);
        for scheduler in [Scheduler::Static, Scheduler::Dynamic] {
            let report = runner::parallel_diagnostics(&games, 2, scheduler).unwrap();
            assert_eq!(report.totals, expected_total);
            check_records(report, &expected_games, 2);
        }
        let after: Vec<_> = games
            .iter()
            .map(|game| (game.state.canonical_bytes(), game.rng.state))
            .collect();
        assert_eq!(after, inputs);
    }
}

fn finished_games() -> Vec<PreparedGame> {
    let bytes = trace::decode_hex(
        include_str!("fixtures/dead-seed191.hex").trim(),
        catan_engine::CANONICAL_SIZE,
    )
    .unwrap();
    let dead = State::from_canonical_bytes(&bytes).unwrap();
    let mut winning = dead;
    winning.config.victory_points = 14;
    let winning = winning.apply_action(1).unwrap();
    assert!(dead.is_dead());
    assert!(winning.is_terminal());
    [dead, winning, dead]
        .into_iter()
        .enumerate()
        .map(|(index, mut state)| {
            state.turn_number += index as u64;
            state.validate().unwrap();
            PreparedGame {
                state,
                rng: SplitMix64::new(index as u64),
            }
        })
        .collect()
}

#[test]
fn scheduling_keeps_dead_and_winner_results_with_empty_worker_partitions() {
    let games = finished_games();
    let (expected_total, expected_games) = serial(&games);
    assert_eq!(expected_total.attempts, 3);
    assert_eq!(expected_total.winners, 1);
    assert_eq!(expected_total.dead_games, 2);
    assert_eq!(expected_total.decisions, 0);
    assert_eq!(expected_total.chance_steps, 0);
    // One worker, an uneven partition, and more workers than input games.
    for threads in [1, 2, 4] {
        assert_eq!(
            runner::parallel_static(&games, threads).unwrap().0,
            expected_total
        );
        assert_eq!(runner::parallel(&games, threads).unwrap().0, expected_total);
        for scheduler in [Scheduler::Static, Scheduler::Dynamic] {
            let report = runner::parallel_diagnostics(&games, threads, scheduler).unwrap();
            assert_eq!(report.totals, expected_total);
            check_records(report, &expected_games, threads);
        }
    }
}

#[test]
fn scheduling_accepts_empty_inputs_and_rejects_zero_workers() {
    for threads in [1, 4] {
        assert_eq!(
            runner::parallel_static(&[], threads).unwrap().0,
            Totals::default()
        );
        assert_eq!(runner::parallel(&[], threads).unwrap().0, Totals::default());
        for scheduler in [Scheduler::Static, Scheduler::Dynamic] {
            let report = runner::parallel_diagnostics(&[], threads, scheduler).unwrap();
            assert_eq!(report.totals, Totals::default());
            check_records(report, &[], threads);
        }
    }
    assert!(runner::parallel_static(&[], 0).is_err());
    assert!(runner::parallel(&[], 0).is_err());
    for scheduler in [Scheduler::Static, Scheduler::Dynamic] {
        assert!(runner::parallel_diagnostics(&[], 0, scheduler).is_err());
    }
}
