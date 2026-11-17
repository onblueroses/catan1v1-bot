//! Uniform random policy and independent-game workers. No rules live here.
use crate::{Board, Config, Error, Outcome, RandomSource, SplitMix64, State, phase};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Name of the policy that `choose` and `play` implement. Benchmark output
/// and the measurement scripts record it.
pub const POLICY: &str = "uniform_legal_actions_exact_integer_chance";

/// One transition: an action id at a decision node or an outcome id at a
/// chance node. The two id namespaces are separate, so the variant matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Action(u16),
    Outcome(u16),
}
impl Choice {
    /// Copy-and-apply through the public checked functions.
    pub fn apply(self, state: &State) -> Result<State, Error> {
        match self {
            Self::Action(id) => state.apply_action(id),
            Self::Outcome(id) => state.apply_outcome(id),
        }
    }
    pub(crate) fn advance(self, state: &mut State) -> Result<(), Error> {
        match self {
            Self::Action(id) => state.advance_action(id),
            Self::Outcome(id) => state.advance_outcome(id),
        }
    }
}

/// Rejection sampling in RandomSource::below_u128 preserves every integer mass.
pub fn sample_outcome(outcomes: &[Outcome], rng: &mut impl RandomSource) -> u16 {
    assert!(!outcomes.is_empty());
    let total = outcomes[0].total;
    debug_assert!(total > 0 && outcomes.iter().all(|o| o.total == total && o.weight > 0));
    debug_assert_eq!(outcomes.iter().map(|o| o.weight).sum::<u128>(), total);
    let mut draw = rng.below_u128(total);
    for outcome in outcomes {
        if draw < outcome.weight {
            return outcome.id;
        }
        draw -= outcome.weight;
    }
    unreachable!("Chance masses must sum to their total")
}

// Uniform rollouts need only the original integer masses, not an Outcome
// record per supported ID. Keep the public enumerator and slice sampler as
// independent references. Preserve the exact u128 bound and ascending scan.
fn sample_chance(state: &State, rng: &mut impl RandomSource) -> u16 {
    match state.phase {
        phase::DICE => {
            let probabilities = state.dice_probabilities();
            let mut draw = rng.below_u128(probabilities.total);
            for (id, &weight) in probabilities.weights.iter().enumerate() {
                if draw < weight {
                    return id as u16;
                }
                draw -= weight;
            }
        }
        phase::STEAL | phase::DEVELOPMENT_DRAW => {
            let (counts, base) = if state.phase == phase::STEAL {
                (&state.resources[1 - state.turn_owner as usize], 11)
            } else {
                (&state.development_deck, 16)
            };
            let total = counts.iter().map(|&count| u128::from(count)).sum();
            let mut draw = rng.below_u128(total);
            for (id, &count) in counts.iter().enumerate() {
                let weight = u128::from(count);
                if draw < weight {
                    return base + id as u16;
                }
                draw -= weight;
            }
        }
        _ => unreachable!("Only chance states enter the chance sampler"),
    }
    unreachable!("Chance masses must sum to their total")
}

/// Enumerate the rules engine, including legal transitions from dead positions.
/// Game runners check E10 separately before requesting another choice.
pub fn choose(state: &State, rng: &mut impl RandomSource) -> Option<Choice> {
    if state.phase == phase::TERMINAL {
        return None;
    }
    if state.actor().is_some() {
        let mut actions = [0; 226];
        let n = state.legal_actions(&mut actions);
        assert!(n > 0, "Nonterminal decision state has no legal actions");
        Some(Choice::Action(actions[rng.below(n as u64) as usize]))
    } else {
        Some(Choice::Outcome(sample_chance(state, rng)))
    }
}

// A rollout owns this initialized buffer for its whole game. legal_actions
// writes the complete returned prefix, so stale suffix entries are never read.
// Keep public choose above independent as the fresh-buffer reference.
pub(crate) fn choose_with_scratch(
    state: &State,
    rng: &mut impl RandomSource,
    actions: &mut [u16; crate::ACTION_COUNT],
) -> Option<Choice> {
    if state.phase == phase::TERMINAL {
        return None;
    }
    if state.actor().is_some() {
        let n = state.legal_actions(actions);
        assert!(n > 0, "Nonterminal decision state has no legal actions");
        Some(Choice::Action(actions[rng.below(n as u64) as usize]))
    } else {
        Some(Choice::Outcome(sample_chance(state, rng)))
    }
}

/// One indexed seed per game; scheduling and thread count cannot change a game.
pub fn game_seed(seed: u64, index: u64) -> u64 {
    SplitMix64::new(seed.wrapping_add(index.wrapping_mul(0x9e3779b97f4a7c15))).next_u64()
}

/// A constructor state on a generated board, with the generator positioned
/// after board generation. The same generator then drives the game.
#[derive(Clone, Copy)]
pub struct PreparedGame {
    pub state: State,
    pub rng: SplitMix64,
}
/// Game `index` of the cohort with root `seed`. Benchmarks prepare all games
/// before timing starts.
pub fn prepare(seed: u64, index: u64, config: Config) -> Result<PreparedGame, Error> {
    let mut rng = SplitMix64::new(game_seed(seed, index));
    let board = Board::generate(&config, &mut rng);
    Ok(PreparedGame {
        state: State::new(board, config)?,
        rng,
    })
}

/// Counters of one game or of many games combined. Combining is commutative,
/// so the totals do not depend on how games are split between threads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    /// Games started. `play` returns only at a win or a dead position, so
    /// attempts and games are equal in its totals.
    pub attempts: u64,
    /// Reached results: wins plus E10 dead positions.
    pub games: u64,
    pub decisions: u64,
    pub chance_steps: u64,
    pub turns: u64,
    pub winners: u64,
    pub dead_games: u64,
    pub winner_counts: [u64; 2],
    /// XOR of the final state hashes. It detects any change of game content.
    pub checksum: u64,
}
impl Totals {
    pub fn combine(&mut self, other: Self) {
        self.attempts += other.attempts;
        self.games += other.games;
        self.decisions += other.decisions;
        self.chance_steps += other.chance_steps;
        self.turns += other.turns;
        self.winners += other.winners;
        self.dead_games += other.dead_games;
        for p in 0..2 {
            self.winner_counts[p] += other.winner_counts[p];
        }
        self.checksum ^= other.checksum;
    }
}

/// Play one game under the uniform policy until a win or a dead position
/// (E10). There is no step or turn cap. This loop is the timed path of the
/// `bench` throughput figures.
pub fn play(mut game: PreparedGame) -> Result<Totals, Error> {
    let mut totals = Totals::default();
    let mut actions = [0; crate::ACTION_COUNT];
    while !game.state.is_dead() {
        let Some(choice) = game
            .state
            .choose_and_advance_with_scratch(&mut game.rng, &mut actions)?
        else {
            break;
        };
        match choice {
            Choice::Action(_) => totals.decisions += 1,
            Choice::Outcome(_) => totals.chance_steps += 1,
        }
    }
    totals.attempts = 1;
    totals.turns = game.state.turn_number;
    totals.winners = u64::from(game.state.winner < 2);
    totals.dead_games = u64::from(game.state.is_dead());
    totals.games = totals.winners + totals.dead_games;
    if game.state.winner < 2 {
        totals.winner_counts[game.state.winner as usize] = 1;
    }
    totals.checksum = game.state.hash();
    Ok(totals)
}

/// Original contiguous partitioning, retained as the scheduling control.
/// Boards and constructors precede timing. Launch/join and policy RNG are timed.
pub fn parallel_static(
    games: &[PreparedGame],
    threads: usize,
) -> Result<(Totals, Duration), String> {
    if threads == 0 {
        return Err("threads must be positive".into());
    }
    let start = Instant::now();
    let totals = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(threads);
        for worker in 0..threads {
            // Contiguous balanced batches, including empty ones when games < threads.
            let from = games.len() / threads * worker + (games.len() % threads).min(worker);
            let to = games.len() / threads * (worker + 1) + (games.len() % threads).min(worker + 1);
            let batch = &games[from..to];
            handles.push(scope.spawn(move || {
                let mut local = Totals::default();
                for &game in batch {
                    local.combine(play(game).map_err(|e| format!("Engine error: {e:?}"))?);
                }
                Ok::<_, String>(local)
            }));
        }
        let mut total = Totals::default();
        for handle in handles {
            total.combine(handle.join().map_err(|_| "Worker panicked".to_string())??);
        }
        Ok::<_, String>(total)
    })?;
    Ok((totals, start.elapsed()))
}

/// How games are assigned to worker threads. The results are identical under
/// both. Only the wall time differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheduler {
    /// Each worker gets one contiguous slice of the games.
    Static,
    /// Each worker claims the next unplayed game. This is the default,
    /// because game lengths vary and static slices finish unevenly.
    Dynamic,
}
impl Scheduler {
    pub fn name(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Dynamic => "dynamic",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexedGameResult {
    pub index: usize,
    pub totals: Totals,
}

#[derive(Debug)]
pub struct WorkerDiagnostics {
    pub worker: usize,
    pub elapsed: Duration,
    pub totals: Totals,
    pub games: Vec<IndexedGameResult>,
}

#[derive(Debug)]
pub struct ParallelDiagnostics {
    pub totals: Totals,
    pub elapsed: Duration,
    pub workers: Vec<WorkerDiagnostics>,
}

/// Claim one prepared game at a time. Only claims share an atomic: each complete
/// game retains its own State, RNG and counters, with no per-decision sharing.
/// Boards precede timing. Claims, thread launch/join and final hashes are timed.
pub fn parallel(games: &[PreparedGame], threads: usize) -> Result<(Totals, Duration), String> {
    let report = scheduled::<false>(games, threads, Scheduler::Dynamic)?;
    Ok((report.totals, report.elapsed))
}

/// Separate diagnostic/control run. Record every game index/result and each
/// worker's local duration. Recording costs are timed, so do not use its elapsed
/// value as an uninstrumented throughput measurement. No transition is observed.
pub fn parallel_diagnostics(
    games: &[PreparedGame],
    threads: usize,
    scheduler: Scheduler,
) -> Result<ParallelDiagnostics, String> {
    scheduled::<true>(games, threads, scheduler)
}

/// The shared worker loop. `RECORD` is a compile-time switch, so the
/// uninstrumented instance contains no recording code.
fn scheduled<const RECORD: bool>(
    games: &[PreparedGame],
    threads: usize,
    scheduler: Scheduler,
) -> Result<ParallelDiagnostics, String> {
    if threads == 0 {
        return Err("threads must be positive".into());
    }
    let start = Instant::now();
    let next = AtomicUsize::new(0);
    let (totals, workers) = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(threads);
        for worker in 0..threads {
            let next = &next;
            handles.push(scope.spawn(move || {
                let worker_start = RECORD.then(Instant::now);
                let mut local = Totals::default();
                let mut records = Vec::new();
                let mut run = |index| -> Result<(), String> {
                    let result = play(games[index]).map_err(|e| format!("Engine error: {e:?}"))?;
                    local.combine(result);
                    if RECORD {
                        records.push(IndexedGameResult {
                            index,
                            totals: result,
                        });
                    }
                    Ok(())
                };
                match scheduler {
                    Scheduler::Static => {
                        let from =
                            games.len() / threads * worker + (games.len() % threads).min(worker);
                        let to = games.len() / threads * (worker + 1)
                            + (games.len() % threads).min(worker + 1);
                        for index in from..to {
                            run(index)?;
                        }
                    }
                    Scheduler::Dynamic => loop {
                        // Prepared inputs are immutable and published by scoped spawn.
                        // The index grants uniqueness, not access to mutable shared data.
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= games.len() {
                            break;
                        }
                        run(index)?;
                    },
                }
                Ok::<_, String>(WorkerDiagnostics {
                    worker,
                    elapsed: worker_start.map(|s| s.elapsed()).unwrap_or_default(),
                    totals: local,
                    games: records,
                })
            }));
        }
        let mut total = Totals::default();
        let mut workers = Vec::new();
        for handle in handles {
            let worker = handle.join().map_err(|_| "Worker panicked".to_string())??;
            total.combine(worker.totals);
            if RECORD {
                workers.push(worker);
            }
        }
        Ok::<_, String>((total, workers))
    })?;
    Ok(ParallelDiagnostics {
        totals,
        elapsed: start.elapsed(),
        workers,
    })
}

/// Path counts of an exhaustive expansion to a fixed depth.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Perft {
    pub leaves: u128,
    pub decision_edges: u128,
    pub chance_edges: u128,
    pub terminal_leaves: u128,
}
/// Depth counts all explicit transitions, including chance. Probabilities do not
/// multiply counts: each positive-mass outcome is one branch. Dead positions keep
/// all legal branches because E10 does not change the rules engine.
pub fn perft(state: &State, depth: u32) -> Result<Perft, String> {
    if depth == 0 || state.phase == phase::TERMINAL {
        return Ok(Perft {
            leaves: 1,
            terminal_leaves: u128::from(state.phase == phase::TERMINAL),
            ..Perft::default()
        });
    }
    let mut result = Perft::default();
    let mut visit = |choice: Choice| -> Result<(), String> {
        let child = choice
            .apply(state)
            .map_err(|e| format!("{choice:?}: {e:?}"))?;
        let subtree = perft(&child, depth - 1)?;
        for (a, b) in [
            (&mut result.leaves, subtree.leaves),
            (&mut result.decision_edges, subtree.decision_edges),
            (&mut result.chance_edges, subtree.chance_edges),
            (&mut result.terminal_leaves, subtree.terminal_leaves),
        ] {
            *a = a.checked_add(b).ok_or("perft count exceeds u128")?;
        }
        let edge = match choice {
            Choice::Action(_) => &mut result.decision_edges,
            Choice::Outcome(_) => &mut result.chance_edges,
        };
        *edge = edge.checked_add(1).ok_or("perft count exceeds u128")?;
        Ok(())
    };
    if state.actor().is_some() {
        let mut actions = [0; 226];
        let n = state.legal_actions(&mut actions);
        for &id in &actions[..n] {
            visit(Choice::Action(id))?;
        }
    } else {
        let mut outcomes = [Outcome::default(); 11];
        let n = state.chance_outcomes(&mut outcomes);
        for o in &outcomes[..n] {
            visit(Choice::Outcome(o.id))?;
        }
    }
    Ok(result)
}
