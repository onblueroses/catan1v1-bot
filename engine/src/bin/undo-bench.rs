//! E5 paired branching traversal and recorded-transition microbenchmarks.
use catan_engine::{
    Config, Outcome, State, UndoRecord,
    runner::{self, Choice},
    trace,
};
use serde_json::json;
use std::{fs::File, hint::black_box, io::BufReader, time::Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counts {
    leaves: u128,
    decisions: u128,
    chances: u128,
    checksum: u64,
}
impl Counts {
    fn add(&mut self, child: Self, chance: bool) -> Result<(), String> {
        self.leaves = self
            .leaves
            .checked_add(child.leaves)
            .ok_or("Leaf count overflow")?;
        self.decisions = self
            .decisions
            .checked_add(child.decisions)
            .and_then(|n| n.checked_add(u128::from(!chance)))
            .ok_or("Decision count overflow")?;
        self.chances = self
            .chances
            .checked_add(child.chances)
            .and_then(|n| n.checked_add(u128::from(chance)))
            .ok_or("Chance count overflow")?;
        self.checksum = self.checksum.rotate_left(7).wrapping_add(child.checksum);
        Ok(())
    }
    fn same_work(self, other: Self) -> bool {
        self.leaves == other.leaves
            && self.decisions == other.decisions
            && self.chances == other.chances
    }
}
fn choices(state: &State, out: &mut [Choice; 226]) -> usize {
    if state.is_chance() {
        let mut outcomes = [Outcome::default(); 11];
        let n = state.chance_outcomes(&mut outcomes);
        for (choice, outcome) in out.iter_mut().zip(&outcomes[..n]) {
            *choice = Choice::Outcome(outcome.id);
        }
        n
    } else {
        let mut actions = [0; 226];
        let n = state.legal_actions(&mut actions);
        for (choice, &action) in out.iter_mut().zip(&actions[..n]) {
            *choice = Choice::Action(action);
        }
        n
    }
}
fn leaf<const VERIFY: bool>(state: &State) -> Counts {
    black_box(state);
    Counts {
        leaves: 1,
        checksum: if VERIFY { state.hash() } else { 0 },
        ..Counts::default()
    }
}
fn copy_tree<const VERIFY: bool>(state: &State, depth: u32) -> Result<Counts, String> {
    if depth == 0 || state.is_terminal() {
        return Ok(leaf::<VERIFY>(state));
    }
    let mut list = [Choice::Action(0); 226];
    let n = choices(state, &mut list);
    let mut result = Counts::default();
    for &choice in &list[..n] {
        let child = black_box(choice)
            .apply(black_box(state))
            .map_err(|e| e.to_string())?;
        let counts = copy_tree::<VERIFY>(black_box(&child), depth - 1)?;
        result.add(counts, matches!(choice, Choice::Outcome(_)))?;
    }
    Ok(result)
}
fn undo_tree<const VERIFY: bool>(state: &mut State, depth: u32) -> Result<Counts, String> {
    if depth == 0 || state.is_terminal() {
        return Ok(leaf::<VERIFY>(state));
    }
    let mut list = [Choice::Action(0); 226];
    let n = choices(state, &mut list);
    let mut result = Counts::default();
    for &choice in &list[..n] {
        // VERIFY is a compile-time constant. No State snapshot, copy oracle,
        // canonical serialization or cache verification enters the timed path.
        let before = if VERIFY { Some(*state) } else { None };
        let expected = if VERIFY {
            Some(choice.apply(state).map_err(|e| e.to_string())?)
        } else {
            None
        };
        let record = black_box(choice)
            .make(black_box(state))
            .map_err(|e| e.to_string())?;
        if VERIFY {
            if Some(*state) != expected {
                return Err(format!("Sparse successor differs for {choice:?}"));
            }
            state
                .validate()
                .map_err(|e| format!("Sparse successor cache/invariant error: {e}"))?;
        }
        let child = undo_tree::<VERIFY>(black_box(state), depth - 1);
        state.unmake(black_box(record));
        black_box(&mut *state);
        if VERIFY
            && (Some(*state) != before
                || state.canonical_bytes() != before.unwrap().canonical_bytes())
        {
            return Err(format!("Sparse restoration differs for {choice:?}"));
        }
        result.add(child?, matches!(choice, Choice::Outcome(_)))?;
    }
    Ok(result)
}
fn traversal(roots: &[State], depth: u32, repeats: u64) -> Result<(), String> {
    let mut expected = Vec::new();
    for root in roots {
        root.validate().map_err(|e| e.to_string())?;
        let copy = copy_tree::<true>(root, depth)?;
        let mut current = *root;
        let undo = undo_tree::<true>(&mut current, depth)?;
        if copy != undo || current != *root {
            return Err("Copy and undo trees or restored roots differ".into());
        }
        expected.push(copy);
    }
    // Alternate method order between caller-selected repetitions. All
    // verification above precedes timing. The output preserves each sample.
    for repetition in 0..repeats {
        for method in if repetition % 2 == 0 {
            ["copy", "sparse_undo"]
        } else {
            ["sparse_undo", "copy"]
        } {
            let mut current = roots.to_vec();
            let mut results = Vec::with_capacity(roots.len());
            let start = Instant::now();
            if method == "copy" {
                for root in roots {
                    results.push(copy_tree::<false>(black_box(root), depth)?);
                }
            } else {
                for root in &mut current {
                    results.push(undo_tree::<false>(black_box(root), depth)?);
                }
            }
            let seconds = start.elapsed().as_secs_f64();
            black_box(&results);
            if results
                .iter()
                .zip(&expected)
                .any(|(&a, &b)| !a.same_work(b))
                || current != roots
            {
                return Err("Timed traversal changed work or failed to restore roots".into());
            }
            println!(
                "{}",
                json!({"measurement":"e5_branching", "method":method,
                "repetition":repetition.to_string(),"depth":depth,"roots":roots.len(),"seconds":seconds,
                "state_bytes":size_of::<State>(),"undo_record_bytes":size_of::<UndoRecord>(),
                "root_counts":expected.iter().map(|c| json!({"leaves":c.leaves.to_string(),
                    "decision_edges":c.decisions.to_string(),"chance_edges":c.chances.to_string(),
                    "leaf_hash":format!("{:016x}",c.checksum)})).collect::<Vec<_>>(),
                "includes":"checked transitions, legal/chance enumeration, traversal counts, make/unmake for sparse method",
                "excludes":"root copies, canonical hashing, cache validation, restoration/equality verification"})
            );
        }
    }
    Ok(())
}
fn micro(corpus: &[(State, Choice)], repeats: u64) -> Result<(), String> {
    if corpus.is_empty() {
        return Err("Trace corpus contains no normal-play transitions".into());
    }
    let mut current: Vec<_> = corpus.iter().map(|&(s, _)| s).collect();
    let mut records: Vec<Option<UndoRecord>> =
        std::iter::repeat_with(|| None).take(corpus.len()).collect();
    let mut phases = [0usize; 14];
    for &(state, choice) in corpus {
        phases[state.phase as usize] += 1;
        let expected = choice.apply(&state).map_err(|e| e.to_string())?;
        let mut mutable = state;
        let record = choice.make(&mut mutable).map_err(|e| e.to_string())?;
        if mutable != expected {
            return Err("Micro successor differs".into());
        }
        mutable.validate().map_err(|e| e.to_string())?;
        mutable.unmake(record);
        if mutable != state {
            return Err("Micro restoration differs".into());
        }
    }
    let mut copy_seconds = 0.0;
    let mut pair_seconds = 0.0;
    let mut make_seconds = 0.0;
    let mut unmake_seconds = 0.0;
    for repetition in 0..repeats {
        for method in if repetition % 2 == 0 {
            ["copy", "sparse_undo"]
        } else {
            ["sparse_undo", "copy"]
        } {
            let start = Instant::now();
            if method == "copy" {
                for (state, choice) in corpus {
                    let child = black_box(*choice)
                        .apply(black_box(state))
                        .map_err(|e| e.to_string())?;
                    black_box(&child);
                }
                copy_seconds += start.elapsed().as_secs_f64();
            } else {
                for ((_, choice), state) in corpus.iter().zip(&mut current) {
                    let record = black_box(*choice)
                        .make(black_box(&mut *state))
                        .map_err(|e| e.to_string())?;
                    black_box(&mut *state);
                    state.unmake(black_box(record));
                    black_box(&mut *state);
                }
                pair_seconds += start.elapsed().as_secs_f64();
            }
        }
        // Separate make-only and unmake-only intervals use the same mutable
        // corpus and reusable record slots. No State copy prepares a timed make
        // or unmake, and each move-only record is consumed exactly once.
        let start = Instant::now();
        for (((_, choice), state), record) in corpus.iter().zip(&mut current).zip(&mut records) {
            *record = Some(
                black_box(*choice)
                    .make(black_box(&mut *state))
                    .map_err(|e| e.to_string())?,
            );
            black_box(&mut *state);
            black_box(&mut *record);
        }
        make_seconds += start.elapsed().as_secs_f64();
        let start = Instant::now();
        for (state, record) in current.iter_mut().zip(&mut records) {
            state.unmake(black_box(record.take().unwrap()));
            black_box(&mut *state);
        }
        unmake_seconds += start.elapsed().as_secs_f64();
        if current.iter().zip(corpus).any(|(a, (b, _))| a != b) {
            return Err("Timed microbenchmark did not restore inputs".into());
        }
    }
    let scale = 1e9 / (repeats as f64 * corpus.len() as f64);
    println!(
        "{}",
        json!({"measurement":"e5_micro","samples":corpus.len(),"repetitions":repeats.to_string(),
        "phase_counts":phases,"state_bytes":size_of::<State>(),"undo_record_bytes":size_of::<UndoRecord>(),
        "copy_apply_ns":copy_seconds*scale,"sparse_make_unmake_ns":pair_seconds*scale,
        "sparse_make_ns":make_seconds*scale,"sparse_unmake_ns":unmake_seconds*scale,
        "excludes":"corpus preparation, full-state copies in sparse loop, hashing, validation, equality checks"})
    );
    Ok(())
}
fn run() -> Result<(), String> {
    let mut depth = None;
    let mut repeats = None;
    let mut seed = 0;
    let mut config = Config::default();
    let mut snapshots = Vec::new();
    let mut paths = Vec::new();
    let mut indices = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" {
            println!(
                "undo-bench --depth N --repeats N [--seed N] [--dice balanced|independent] [--snapshot FILE ...] [--trace TRACE ... --root-index N ...]\nDepth and repetitions are caller choices. Every positive-mass chance branch is expanded. Traces are validated before timing and supply the normal-play micro corpus. Root indices address that corpus in input order. Supply explicit root indices with traces, or explicit binary snapshots. With neither, use the constructor root. Run measurements on a dedicated benchmark machine."
            );
            return Ok(());
        }
        let value = args
            .next()
            .ok_or_else(|| format!("Missing value for {arg}"))?;
        match arg.as_str() {
            "--depth" => depth = Some(value.parse::<u32>().map_err(|_| "Invalid depth")?),
            "--repeats" => repeats = Some(value.parse::<u64>().map_err(|_| "Invalid repeats")?),
            "--seed" => seed = value.parse().map_err(|_| "Invalid seed")?,
            "--dice" => {
                config.dice = match value.as_str() {
                    "balanced" => 0,
                    "independent" => 1,
                    _ => return Err("Invalid dice".into()),
                }
            }
            "--snapshot" => snapshots.push(value),
            "--trace" => paths.push(value),
            "--root-index" => {
                indices.push(value.parse::<usize>().map_err(|_| "Invalid root index")?)
            }
            _ => return Err(format!("Unknown option {arg}")),
        }
    }
    let depth = depth.ok_or("--depth is required")?;
    let repeats = repeats
        .filter(|&n| n > 0)
        .ok_or("--repeats must be supplied and positive")?;
    let mut corpus = Vec::new();
    for path in &paths {
        trace::replay_with(
            BufReader::new(File::open(path).map_err(|e| format!("{path}: {e}"))?),
            |s, c| {
                if s.turn_number > 0 {
                    corpus.push((*s, c));
                }
            },
        )
        .map_err(|e| format!("{path}: {e}"))?;
    }
    let mut roots = Vec::new();
    for path in snapshots {
        roots.push(
            State::from_canonical_bytes(&std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?)
                .map_err(|e| format!("{path}: {e}"))?,
        );
    }
    for index in indices {
        roots.push(
            corpus
                .get(index)
                .ok_or_else(|| format!("Root index {index} outside corpus of {}", corpus.len()))?
                .0,
        );
    }
    if roots.is_empty() {
        if !paths.is_empty() {
            return Err(
                "Traces require explicit --root-index or --snapshot traversal roots".into(),
            );
        }
        roots.push(
            runner::prepare(seed, 0, config)
                .map_err(|e| e.to_string())?
                .state,
        );
    }
    println!(
        "{}",
        json!({"measurement":"e5_inputs","traces":paths,"root_hashes":roots.iter().map(|s| format!("{:016x}",s.hash())).collect::<Vec<_>>(),
        "root_phases":roots.iter().map(|s| s.phase).collect::<Vec<_>>(),"state_bytes":size_of::<State>(),"undo_record_bytes":size_of::<UndoRecord>()})
    );
    traversal(&roots, depth, repeats)?;
    if !paths.is_empty() {
        micro(&corpus, repeats)?;
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("undo-bench: {error}");
        std::process::exit(1);
    }
}
