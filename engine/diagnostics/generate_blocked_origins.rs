//! Standalone diagnostic, link against the corrected native engine rlib.
//! Arguments: output-directory captured-fixture-directory. No gameplay cap.
use catan_engine::{CANONICAL_SIZE, State, runner, trace};
use std::{
    env,
    fs::{self, OpenOptions},
    io::{BufWriter, Write},
    path::PathBuf,
};

const ORIGINS: [(u64, &str); 11] = [
    (9116, "0a90558550ad22c7"),
    (10842, "d8c057c2ffe00dc9"),
    (16875, "0c82494c4d35564b"),
    (17249, "25802412ac6f9095"),
    (18287, "843b77884e142c57"),
    (40740, "a9f1431a136bf4ec"),
    (46545, "2240b8f5f20f7c5a"),
    (50386, "3c73151b339f0bf8"),
    (53216, "6377fcc1cce7e44e"),
    (57787, "965e934cf9aa62d5"),
    (58986, "15c41219edbaff81"),
];
fn main() {
    let args = env::args().collect::<Vec<_>>();
    let out = PathBuf::from(&args[1]);
    let fixtures = PathBuf::from(&args[2]);
    fs::create_dir_all(&out).unwrap();
    for (index, hash) in ORIGINS {
        let encoded =
            fs::read_to_string(fixtures.join(format!("blocked-roads-{hash}.hex"))).unwrap();
        let captured = State::from_canonical_bytes(
            &trace::decode_hex(encoded.trim(), CANONICAL_SIZE).unwrap(),
        )
        .unwrap();
        assert!(
            captured.is_dead(),
            "Refuse origin replay under the old incomplete certificate"
        );
        assert_eq!(format!("{:016x}", captured.hash()), hash);
        let mut game = runner::prepare(41, index, captured.config).unwrap();
        assert_eq!(
            game.state.board, captured.board,
            "Captured immutable board must match exact prepare(seed=41,index)"
        );
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(out.join(format!("balanced-seed41-index{index}.jsonl")))
            .unwrap();
        let mut writer = BufWriter::new(file);
        let first_dead =
            trace::generate(&mut writer, game.state, &mut game.rng, false, None).unwrap();
        writer.flush().unwrap();
        assert!(first_dead.is_dead() && !first_dead.is_terminal());
        assert_eq!(first_dead.node_occupancy, captured.node_occupancy);
        assert_eq!(first_dead.edge_occupancy, captured.edge_occupancy);
        assert_eq!(first_dead.cache.total_points, captured.cache.total_points);
        assert_eq!(first_dead.largest_army_holder, captured.largest_army_holder);
        assert_eq!(first_dead.longest_road_holder, captured.longest_road_holder);
        println!(
            "index={index} first_dead_hash={:016x} turns={} captured_hash={hash}",
            first_dead.hash(),
            first_dead.turn_number
        );
    }
}
