//! Independent graph and accounting oracles over complete generated games.
use catan_engine::*;
#[path = "support/chance_oracle.rs"]
mod chance_oracle;
#[path = "support/geometry_oracle.rs"]
mod geometry_oracle;
const EDGES: [[usize; 2]; 72] = [
    [0, 3],
    [0, 4],
    [1, 4],
    [1, 5],
    [2, 5],
    [2, 6],
    [3, 7],
    [4, 8],
    [5, 9],
    [6, 10],
    [7, 11],
    [7, 12],
    [8, 12],
    [8, 13],
    [9, 13],
    [9, 14],
    [10, 14],
    [10, 15],
    [11, 16],
    [12, 17],
    [13, 18],
    [14, 19],
    [15, 20],
    [16, 21],
    [16, 22],
    [17, 22],
    [17, 23],
    [18, 23],
    [18, 24],
    [19, 24],
    [19, 25],
    [20, 25],
    [20, 26],
    [21, 27],
    [22, 28],
    [23, 29],
    [24, 30],
    [25, 31],
    [26, 32],
    [27, 33],
    [28, 33],
    [28, 34],
    [29, 34],
    [29, 35],
    [30, 35],
    [30, 36],
    [31, 36],
    [31, 37],
    [32, 37],
    [33, 38],
    [34, 39],
    [35, 40],
    [36, 41],
    [37, 42],
    [38, 43],
    [39, 43],
    [39, 44],
    [40, 44],
    [40, 45],
    [41, 45],
    [41, 46],
    [42, 46],
    [43, 47],
    [44, 48],
    [45, 49],
    [46, 50],
    [47, 51],
    [48, 51],
    [48, 52],
    [49, 52],
    [49, 53],
    [50, 53],
];
const HEXES: [[usize; 6]; 19] = [
    [0, 4, 8, 12, 7, 3],
    [1, 5, 9, 13, 8, 4],
    [2, 6, 10, 14, 9, 5],
    [7, 12, 17, 22, 16, 11],
    [8, 13, 18, 23, 17, 12],
    [9, 14, 19, 24, 18, 13],
    [10, 15, 20, 25, 19, 14],
    [16, 22, 28, 33, 27, 21],
    [17, 23, 29, 34, 28, 22],
    [18, 24, 30, 35, 29, 23],
    [19, 25, 31, 36, 30, 24],
    [20, 26, 32, 37, 31, 25],
    [28, 34, 39, 43, 38, 33],
    [29, 35, 40, 44, 39, 34],
    [30, 36, 41, 45, 40, 35],
    [31, 37, 42, 46, 41, 36],
    [39, 44, 48, 51, 47, 43],
    [40, 45, 49, 52, 48, 44],
    [41, 46, 50, 53, 49, 45],
];
const PORTS: [[usize; 2]; 9] = [
    [1, 5],
    [10, 15],
    [26, 32],
    [42, 46],
    [49, 52],
    [47, 51],
    [33, 38],
    [11, 16],
    [0, 3],
];
fn owner(building: u8) -> Option<usize> {
    if building == 0 {
        None
    } else {
        Some((building as usize - 1) % 2)
    }
}
fn trail(s: &State, p: usize) -> u8 {
    fn walk(s: &State, p: usize, n: usize, used: u128) -> u8 {
        if used != 0 && owner(s.node_occupancy[n]).is_some_and(|q| q != p) {
            return 0;
        }
        EDGES
            .iter()
            .enumerate()
            .filter(|(e, ends)| {
                s.edge_occupancy[*e] == p as u8 + 1 && used & (1u128 << e) == 0 && ends.contains(&n)
            })
            .map(|(e, ends)| 1 + walk(s, p, ends[0] + ends[1] - n, used | (1u128 << e)))
            .max()
            .unwrap_or(0)
    }
    (0..54).map(|n| walk(s, p, n, 0)).max().unwrap()
}
fn check(s: &State) {
    let dead = s.is_dead();
    assert_eq!(geometry_oracle::EDGES, EDGES);
    geometry_oracle::assert_geometry(s, &EDGES);
    for r in 0..5 {
        assert_eq!(
            u16::from(s.bank[r]) + u16::from(s.resources[0][r]) + u16::from(s.resources[1][r]),
            19,
            "resource {r}"
        );
    }
    for t in 0..5 {
        let sum = u16::from(s.development_deck[t])
            + (0..2)
                .map(|p| {
                    u16::from(s.playable_development[p][t])
                        + u16::from(s.new_development[p][t])
                        + u16::from(s.spent_development[p][t])
                })
                .sum::<u16>();
        assert_eq!(sum, [14, 5, 2, 2, 2][t], "development {t}");
    }
    let mut production = [[[0u8; 5]; 2]; 11];
    for p in 0..2 {
        let roads = s
            .edge_occupancy
            .iter()
            .filter(|&&o| o == p as u8 + 1)
            .count();
        let settlements = s
            .node_occupancy
            .iter()
            .filter(|&&o| o == p as u8 + 1)
            .count();
        let cities = s
            .node_occupancy
            .iter()
            .filter(|&&o| o == p as u8 + 3)
            .count();
        assert!(roads <= 15 && settlements <= 5 && cities <= 4);
        assert_eq!(
            s.cache.pieces_left[p],
            [
                (15 - roads) as u8,
                (5 - settlements) as u8,
                (4 - cities) as u8
            ]
        );
        let visible = settlements as u8
            + 2 * cities as u8
            + 2 * u8::from(s.largest_army_holder == p as u8)
            + 2 * u8::from(s.longest_road_holder == p as u8);
        assert_eq!(s.cache.visible_points[p], visible);
        assert_eq!(
            s.cache.total_points[p],
            visible + s.playable_development[p][1] + s.new_development[p][1]
        );
        assert_eq!(
            s.cache.resource_totals[p],
            s.resources[p].iter().sum::<u8>()
        );
        assert_eq!(s.cache.road_lengths[p], trail(s, p));
        let mut rates = [4; 5];
        for (slot, nodes) in PORTS.iter().enumerate() {
            if nodes.iter().any(|&n| owner(s.node_occupancy[n]) == Some(p)) {
                let t = s.board.port_type[slot];
                if t == 5 {
                    for rate in &mut rates {
                        *rate = (*rate).min(3);
                    }
                } else {
                    rates[t as usize] = 2;
                }
            }
        }
        assert_eq!(s.cache.trade_rates[p], rates);
    }
    for (h, nodes) in HEXES.iter().enumerate() {
        if h == s.robber_hex as usize || s.board.terrain[h] == 5 {
            continue;
        }
        for &n in nodes {
            if let Some(p) = owner(s.node_occupancy[n]) {
                production[s.board.number[h] as usize - 2][p][s.board.terrain[h] as usize] +=
                    if s.node_occupancy[n] >= 3 { 2 } else { 1 };
            }
        }
    }
    assert_eq!(s.cache.production, production);
    let bytes = s.canonical_bytes();
    let restored = State::from_canonical_bytes(&bytes).unwrap();
    assert_eq!(restored.canonical_bytes(), bytes);
    assert_eq!(
        restored.cache, s.cache,
        "entire cache versus raw decoder rebuild"
    );
    let mut hash = 14695981039346656037u64;
    for b in bytes {
        hash = (hash ^ u64::from(b)).wrapping_mul(1099511628211);
    }
    assert_eq!(s.hash(), hash);
    let mut actions = [0; 226];
    let n = s.legal_actions(&mut actions);
    assert!(actions[..n].windows(2).all(|w| w[0] < w[1]));
    for id in 0..226 {
        assert_eq!(
            s.is_legal_action(id),
            actions[..n].contains(&id),
            "scalar/action buffer {id}"
        );
        let successor = s.apply_action(id);
        let mut sparse = *s;
        match sparse.make_action(id) {
            Ok(record) => {
                assert_eq!(Ok(sparse), successor, "sparse action {id}");
                sparse.unmake(record);
            }
            Err(error) => assert_eq!(Err(error), successor, "sparse action error {id}"),
        }
        assert_eq!(sparse, *s, "exact sparse action restoration {id}");
        assert_eq!(sparse.canonical_bytes(), bytes);
        assert_eq!(
            successor.is_ok(),
            actions[..n].contains(&id),
            "legal/apply id {id} phase {}",
            s.phase
        );
        if dead && let Ok(successor) = successor {
            check_dead_successor(&successor);
        }
    }
    assert!(s.apply_action(226).is_err());
    assert!(s.apply_action(u16::MAX).is_err());
    let mut outcomes = [Outcome::default(); 11];
    let count = s.chance_outcomes(&mut outcomes);
    chance_oracle::assert_outcomes(s, &outcomes[..count]);
    if count > 0 {
        let total = outcomes[0].total;
        assert!(total > 0);
        assert_eq!(
            outcomes[..count].iter().map(|o| o.weight).sum::<u128>(),
            total
        );
        assert!(
            outcomes[..count]
                .iter()
                .all(|o| o.weight > 0 && o.total == total)
        );
        assert!(outcomes[..count].windows(2).all(|w| w[0].id < w[1].id));
        assert_eq!(n, 0);
    } else if s.phase != 13 {
        assert!(n > 0);
    }
    for id in 0..21 {
        let successor = s.apply_outcome(id);
        let mut sparse = *s;
        match sparse.make_outcome(id) {
            Ok(record) => {
                assert_eq!(Ok(sparse), successor, "sparse outcome {id}");
                sparse.unmake(record);
            }
            Err(error) => assert_eq!(Err(error), successor, "sparse outcome error {id}"),
        }
        assert_eq!(sparse, *s, "exact sparse outcome restoration {id}");
        assert_eq!(sparse.canonical_bytes(), bytes);
        assert_eq!(
            successor.is_ok(),
            outcomes[..count].iter().any(|o| o.id == id),
            "chance/apply {id}"
        );
        if dead && let Ok(successor) = successor {
            check_dead_successor(&successor);
        }
    }
    assert_eq!(
        s.canonical_bytes(),
        bytes,
        "pure apply must preserve source, including rejected actions"
    );
}

fn check_dead_successor(successor: &State) {
    successor.validate().unwrap();
    assert!(
        successor.is_dead(),
        "dead must be closed under every successor"
    );
    assert_eq!(successor.winner(), None, "dead cannot lead to a win");
    assert_ne!(successor.phase, phase::TERMINAL);
}

fn check_frozen_seed191_points(s: &State) {
    assert_eq!(s.hash(), 0x4b3fd2661552e6c0);
    // This particular certificate freezes points. E10's general closure law also
    // permits certificates with unreachable winning bounds and changing points.
    for id in 0..ACTION_COUNT as u16 {
        if let Ok(successor) = s.apply_action(id) {
            assert_eq!(successor.cache.total_points, s.cache.total_points);
        }
    }
    for id in 0..21 {
        if let Ok(successor) = s.apply_outcome(id) {
            assert_eq!(successor.cache.total_points, s.cache.total_points);
        }
    }
}

#[test]
fn e10_frozen_seed191_successors_preserve_points() {
    let bytes = trace::decode_hex(
        include_str!("fixtures/dead-seed191.hex").trim(),
        CANONICAL_SIZE,
    )
    .unwrap();
    let s = State::from_canonical_bytes(&bytes).unwrap();
    assert!(s.is_dead());
    assert_eq!(s.cache.total_points, [14, 14]);
    check(&s);
    check_frozen_seed191_points(&s);
}

fn evidence_timestamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

/// Opt-in test instrumentation, independent of the engine and runner. The model
/// stores measurements and the chosen false-alarm rate beside both thresholds.
/// Its nominal tail rate excludes parameter uncertainty and model error.
struct HarnessWatchdog {
    model: serde_json::Value,
    first_mark: u64,
    second_mark: u64,
    marked: bool,
    directory: std::path::PathBuf,
    control: bool,
}

impl HarnessWatchdog {
    fn new(model: serde_json::Value, directory: std::path::PathBuf) -> Self {
        assert_eq!(model["version"], 1);
        assert_eq!(model["policy"], "spending-preferring-uniform");
        let first_mark = model["first_mark_steps"]
            .as_u64()
            .expect("measured first mark");
        let second_mark = model["second_mark_steps"]
            .as_u64()
            .expect("fitted second mark");
        assert!(first_mark > 0 && second_mark > first_mark);
        assert!(
            !model["basis"]
                .as_str()
                .expect("model source")
                .trim()
                .is_empty()
        );
        let alpha = model["nominal_per_game_false_alarm"]
            .as_f64()
            .expect("nominal model rate");
        assert!(alpha > 0.0 && alpha < 1.0);
        assert!(
            model["completed_count"]
                .as_u64()
                .expect("measured sample count")
                > 0
        );
        assert!(
            model["sources"]
                .as_array()
                .is_some_and(|sources| !sources.is_empty())
        );
        Self {
            model,
            first_mark,
            second_mark,
            marked: false,
            directory,
            control: false,
        }
    }

    fn preserve(
        &self,
        stage: &str,
        s: &State,
        rng: &SplitMix64,
        seed: u64,
        steps: u64,
    ) -> std::path::PathBuf {
        std::fs::create_dir_all(&self.directory).unwrap();
        let path = self.directory.join(format!(
            "{stage}-seed-{seed}-steps-{steps}-{}-{}.json",
            std::process::id(),
            evidence_timestamp()
        ));
        let evidence = serde_json::json!({
            "type": "property-watchdog-evidence", "stage": stage, "control": self.control,
            "policy": "spending-preferring-uniform", "seed": seed.to_string(),
            "rng_state": rng.state.to_string(), "steps": steps.to_string(),
            "turns": s.turn_number.to_string(), "dice": s.config.dice, "first_player": s.config.first_player,
            "state_hex": trace::hex(&s.canonical_bytes()), "hash": format!("{:016x}", s.hash()),
            "watchdog_model": self.model,
        });
        // Never replace earlier evidence, including evidence from other runs.
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        serde_json::to_writer_pretty(file, &evidence).unwrap();
        path
    }

    fn inspect(&mut self, s: &State, rng: &SplitMix64, seed: u64, steps: u64) {
        assert!(
            s.winner().is_none() && !s.is_dead(),
            "only unresolved games reach watchdog inspection"
        );
        if steps >= self.first_mark && !self.marked {
            let path = self.preserve("measured-mark", s, rng, seed, steps);
            self.marked = true;
            eprintln!(
                "watchdog=measured-mark seed={seed} steps={steps} first={} second={} action=continue evidence={}",
                self.first_mark,
                self.second_mark,
                path.display()
            );
        }
        if steps >= self.second_mark {
            let path = self.preserve("fitted-tail-failure", s, rng, seed, steps);
            panic!(
                "fitted test watchdog reached without a win or dead result: seed={seed} steps={steps} nominal_alpha={} evidence={}",
                self.model["nominal_per_game_false_alarm"],
                path.display()
            );
        }
    }

    fn finish(&self, s: &State, seed: u64, steps: u64, result: &str) {
        if !self.marked {
            return;
        }
        assert!(matches!(result, "winner" | "dead"));
        assert_eq!(result == "dead", s.is_dead());
        assert_eq!(result == "winner", s.winner().is_some());
        let row = serde_json::json!({
            "type": "property-watchdog-final", "control": self.control,
            "policy": "spending-preferring-uniform", "seed": seed.to_string(), "steps": steps.to_string(),
            "turns": s.turn_number.to_string(), "dice": s.config.dice, "first_player": s.config.first_player,
            "result": result, "winner": s.winner(), "first_mark_steps": self.first_mark,
            "second_mark_steps": self.second_mark, "watchdog_source": self.model["basis"],
        });
        let mut bytes = serde_json::to_vec(&row).unwrap();
        bytes.push(b'\n');
        let mut log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.directory.join("marked-lengths.jsonl"))
            .unwrap();
        std::io::Write::write_all(&mut log, &bytes).unwrap();
        eprintln!(
            "watchdog=marked-final seed={seed} steps={steps} result={result} winner={:?}",
            s.winner()
        );
    }
}

fn harness_watchdog() -> Option<HarnessWatchdog> {
    assert!(
        std::env::var_os("CATAN_PROPERTY_WATCHDOG_STEPS").is_none()
            && std::env::var_os("CATAN_PROPERTY_WATCHDOG_SOURCE").is_none(),
        "single-stage watchdog configuration was replaced. Set CATAN_PROPERTY_WATCHDOG_MODEL to the sourced two-stage model."
    );
    std::env::var_os("CATAN_PROPERTY_WATCHDOG_MODEL").map(|path| {
        let model = serde_json::from_slice(&std::fs::read(path).expect("read watchdog model"))
            .expect("parse watchdog model");
        HarnessWatchdog::new(
            model,
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".local/property-failures"),
        )
    })
}

fn control_watchdog() -> HarnessWatchdog {
    let model = serde_json::from_str(include_str!("fixtures/watchdog-model.json")).unwrap();
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        ".local/property-failures/control-{}-{}",
        std::process::id(),
        evidence_timestamp()
    ));
    let mut watchdog = HarnessWatchdog::new(model, directory);
    // Controls exercise instrumentation with synthetic step counters. Exclude
    // their artifacts from every subsequent length-distribution fit.
    watchdog.control = true;
    watchdog
}

#[test]
fn watchdog_first_mark_continues_and_records_the_final_length() {
    let mut watchdog = control_watchdog();
    let s = State::new(Board::default(), Config::default()).unwrap();
    let rng = SplitMix64::new(206);
    let bytes = s.canonical_bytes();
    watchdog.inspect(&s, &rng, 206, watchdog.first_mark);
    assert!(watchdog.marked);
    assert_eq!(s.canonical_bytes(), bytes);
    let bytes = trace::decode_hex(
        include_str!("fixtures/dead-seed191.hex").trim(),
        CANONICAL_SIZE,
    )
    .unwrap();
    let dead = State::from_canonical_bytes(&bytes).unwrap();
    watchdog.finish(&dead, 206, watchdog.first_mark + 1, "dead");
    let log = std::fs::read_to_string(watchdog.directory.join("marked-lengths.jsonl")).unwrap();
    let row: serde_json::Value = serde_json::from_str(&log).unwrap();
    assert_eq!(row["seed"], "206");
    assert_eq!(row["steps"], (watchdog.first_mark + 1).to_string());
    assert_eq!(row["result"], "dead");
    assert_eq!(row["control"], true);
}

#[test]
fn watchdog_second_mark_fails_and_preserves_the_unresolved_state() {
    let mut watchdog = control_watchdog();
    let s = State::new(Board::default(), Config::default()).unwrap();
    let rng = SplitMix64::new(206);
    watchdog.inspect(&s, &rng, 206, watchdog.second_mark - 1);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        watchdog.inspect(&s, &rng, 206, watchdog.second_mark);
    }));
    assert!(
        failure.is_err(),
        "an unresolved second mark must fail, never pass"
    );
    let path = std::fs::read_dir(&watchdog.directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("fitted-tail-failure-")
        })
        .unwrap();
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(evidence["state_hex"], trace::hex(&s.canonical_bytes()));
    assert_eq!(evidence["rng_state"], rng.state.to_string());
    assert_eq!(evidence["seed"], "206");
    assert_eq!(evidence["steps"], watchdog.second_mark.to_string());
    assert_eq!(evidence["watchdog_model"], watchdog.model);
    assert!(!watchdog.directory.join("marked-lengths.jsonl").exists());
}

/// The seed set covers each first-player and dice-model combination. The game loop
/// has no step/turn cap or unmeasured watchdog. E10 permits a win or a dead result.
/// A policy can legally avoid both forever, so this is evidence about these sampled
/// games, not a universal termination proof. Per-game lengths and marked final
/// results extend the measurements for later test-watchdog fits.
#[test]
fn complete_games_conserve_cards_recompute_caches_and_restore_exact_bytes() {
    let games = std::env::var("CATAN_PROPERTY_GAMES_PER_CONFIG")
        .map(|s| s.parse::<u64>().expect("positive game count"))
        .unwrap_or(1);
    assert!(games > 0);
    let mut watchdog = harness_watchdog();
    let mut winners = [0u64; 2];
    let mut dead_games = 0u64;
    let mut lengths = Vec::new();
    for game in 0..games {
        for dice in 0..2 {
            for first in 0..2 {
                let seed = 1 + game * 4 + u64::from(dice) * 2 + u64::from(first);
                let mut rng = SplitMix64::new(seed);
                let mut config = Config::default();
                config.dice = dice;
                config.first_player = first;
                let board = Board::generate(&config, &mut rng);
                let mut s = State::new(board, config).unwrap();
                let mut steps = 0u64;
                if let Some(watchdog) = &mut watchdog {
                    watchdog.marked = false;
                }
                while s.phase != phase::TERMINAL && !s.is_dead() {
                    check(&s);
                    if let Some(watchdog) = &mut watchdog {
                        watchdog.inspect(&s, &rng, seed, steps);
                    }
                    let saved = s;
                    let bytes = s.canonical_bytes();
                    let mut outcomes = [Outcome::default(); 11];
                    let n = s.chance_outcomes(&mut outcomes);
                    if n > 0 {
                        let total = outcomes[0].total;
                        let mut draw = rng.below_u128(total);
                        let outcome = outcomes[..n]
                            .iter()
                            .find(|o| {
                                if draw < o.weight {
                                    true
                                } else {
                                    draw -= o.weight;
                                    false
                                }
                            })
                            .unwrap();
                        s = s.apply_outcome(outcome.id).unwrap();
                    } else {
                        let mut actions = [0; 226];
                        let n = s.legal_actions(&mut actions);
                        // Prefer spending on permanent pieces. Otherwise sample uniformly. This
                        // avoids a policy dominated by cyclic port trades while retaining random play.
                        let builds: Vec<_> = actions[..n]
                            .iter()
                            .copied()
                            .filter(|a| (7..187).contains(a) || *a == 2)
                            .collect();
                        let choices = if builds.is_empty() {
                            &actions[..n]
                        } else {
                            &builds
                        };
                        let id = choices[rng.below(choices.len() as u64) as usize];
                        s = s.apply_action(id).unwrap();
                    }
                    assert_eq!(saved.canonical_bytes(), bytes);
                    steps += 1;
                }
                check(&s);
                let result = if s.is_dead() {
                    assert_ne!(s.phase, phase::TERMINAL);
                    assert_eq!(s.winner(), None);
                    assert!(
                        s.cache
                            .total_points
                            .iter()
                            .all(|&p| u16::from(p) < s.config.victory_points)
                    );
                    dead_games += 1;
                    "dead"
                } else {
                    assert_eq!(s.phase, phase::TERMINAL);
                    assert!(s.winner < 2);
                    assert!(
                        u16::from(s.cache.total_points[s.winner as usize])
                            >= s.config.victory_points
                    );
                    winners[s.winner as usize] += 1;
                    "winner"
                };
                if let Some(watchdog) = &watchdog {
                    watchdog.finish(&s, seed, steps, result);
                }
                if seed == 191 {
                    // Permanent regression: game 47, independent dice, first player 0.
                    // The matching constructor trace and canonical fixture live in
                    // edge-reachable-dead-seed191.jsonl and fixtures/dead-seed191.hex.
                    assert_eq!((game, dice, first), (47, 1, 0));
                    assert!(s.is_dead());
                    assert_eq!(steps, 1763);
                    assert_eq!(s.cache.total_points, [14, 14]);
                    assert_eq!(s.hash(), 0x4b3fd2661552e6c0);
                    check_frozen_seed191_points(&s);
                }
                lengths.push((steps, s.turn_number));
                eprintln!(
                    "policy=spending-preferring-uniform game={game} seed={seed} dice={dice} first={first} result={result} winner={:?} steps={steps} turns={}",
                    s.winner(),
                    s.turn_number
                );
            }
        }
    }
    assert_eq!(winners.iter().sum::<u64>() + dead_games, games * 4);
    eprintln!(
        "policy=spending-preferring-uniform games={} winners={} winner_counts={winners:?} dead={dead_games} steps_min={} steps_max={} turns_min={} turns_max={}",
        lengths.len(),
        winners.iter().sum::<u64>(),
        lengths.iter().map(|&(steps, _)| steps).min().unwrap(),
        lengths.iter().map(|&(steps, _)| steps).max().unwrap(),
        lengths.iter().map(|&(_, turns)| turns).min().unwrap(),
        lengths.iter().map(|&(_, turns)| turns).max().unwrap(),
    );
}

#[test]
fn b1_b2_b3_generated_maps_have_exact_multisets_and_requested_constraints() {
    for reds in [false, true] {
        for same in [false, true] {
            let mut c = Config::default();
            c.no_adjacent_6_8 = reds;
            c.no_adjacent_same_number = same;
            let mut rng = SplitMix64::new(u64::from(reds) + 2 * u64::from(same));
            let b = Board::generate(&c, &mut rng);
            let mut terrain = [0; 6];
            let mut numbers = [0; 13];
            let mut ports = [0; 6];
            for h in 0..19 {
                terrain[b.terrain[h] as usize] += 1;
                numbers[b.number[h] as usize] += 1;
                assert_eq!(b.terrain[h] == 5, b.number[h] == 0);
            }
            for &p in &b.port_type {
                ports[p as usize] += 1;
            }
            assert_eq!(terrain, [4, 3, 4, 4, 3, 1]);
            assert_eq!(numbers, [1, 0, 1, 2, 2, 2, 2, 0, 2, 2, 2, 2, 1]);
            assert_eq!(ports, [1, 1, 1, 1, 1, 4]);
            for h in 0..19 {
                for k in h + 1..19 {
                    if HEXES[h].iter().filter(|n| HEXES[k].contains(n)).count() == 2 {
                        if reds {
                            assert!(
                                !([6, 8].contains(&b.number[h]) && [6, 8].contains(&b.number[k]))
                            );
                        }
                        if same {
                            assert_ne!(b.number[h], b.number[k]);
                        }
                    }
                }
            }
        }
    }
}
