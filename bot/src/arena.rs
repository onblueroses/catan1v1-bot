//! The reference arena (BOT-SPEC 10).

use crate::board::generate_board;
use crate::decide::decide;
use crate::net::Nets;
use crate::position::Pos;
use crate::rng::Rng;
use crate::search::Settings;
use catan_engine::{Config, State};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Instant;

/// A player in the arena.
#[derive(Clone, Debug)]
pub enum PlayerKind {
    Bot(Settings),
    Random,
    Cmd(String),
}

impl PlayerKind {
    pub fn parse(s: &str, workers: usize) -> Option<PlayerKind> {
        if s == "random" {
            Some(PlayerKind::Random)
        } else if let Some(p) = s.strip_prefix("cmd:") {
            Some(PlayerKind::Cmd(p.to_string()))
        } else {
            Settings::preset(s).map(|mut st| {
                st.workers = workers;
                PlayerKind::Bot(st)
            })
        }
    }
}

/// The parity configuration of BOT-SPEC 2.1.
pub fn parity_config() -> Config {
    Config { shortage_rule: 1, reference_dice_rounding: true, ..Config::default() }
}

pub struct GameRecord {
    pub pair: u64,
    pub seat: u8,
    /// 'A', 'B' or '-'.
    pub winner: char,
    pub turns: u64,
    pub steps: u32,
    pub seconds: [f64; 2],
    pub decisions: [u32; 2],
    /// `aN` / `cN` events.
    pub events: Vec<String>,
    pub board_pos: Pos,
}

impl GameRecord {
    pub fn line(&self) -> String {
        format!(
            "R {} {} {} {} {} {:.2} {:.2} {} {}",
            self.pair, self.seat, self.winner, self.turns, self.steps, self.seconds[0], self.seconds[1],
            self.decisions[0], self.decisions[1]
        )
    }
    /// The line without the two time fields.
    pub fn result_line(&self) -> String {
        format!(
            "R {} {} {} {} {} {} {}",
            self.pair, self.seat, self.winner, self.turns, self.steps, self.decisions[0], self.decisions[1]
        )
    }
}

struct CmdPlayer {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl CmdPlayer {
    fn start(program: &str) -> std::io::Result<CmdPlayer> {
        let mut child = Command::new("sh").arg("-c").arg(program).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()?;
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Ok(CmdPlayer { child, stdin, stdout })
    }
    /// Ask for an action. Accepts a bare id or a JSON object with "action".
    fn ask(&mut self, pos: &Pos, seed: u64) -> Option<u8> {
        let msg = serde_json::json!({
            "position": {"state": pos.state_hex(), "protocol": pos.protocol()},
            "rng_seed": seed.to_string(),
        });
        writeln!(self.stdin, "{msg}").ok()?;
        self.stdin.flush().ok()?;
        let mut line = String::new();
        self.stdout.read_line(&mut line).ok()?;
        let t = line.trim();
        if let Ok(a) = t.parse::<u8>() {
            return Some(a);
        }
        let v: serde_json::Value = serde_json::from_str(t).ok()?;
        v["action"].as_u64().and_then(|a| u8::try_from(a).ok())
    }
}

impl Drop for CmdPlayer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Play one arena game: pair p, player A at seat s.
pub fn play_game(nets: &Nets, a: &PlayerKind, b: &PlayerKind, pair: u64, seat: u8) -> GameRecord {
    let seed = pair.wrapping_mul(1000003);
    let mut board_rng = Rng::seed(seed.wrapping_add(1));
    let (board, ring) = generate_board(&mut board_rng);
    let mut chance = Rng::seed(seed.wrapping_add(2));
    let st = State::new(board, parity_config()).expect("arena board");
    let mut pos = Pos::initial(st, ring, 10_000);
    let start = pos;
    let players = [a, b];
    let mut cmds: [Option<CmdPlayer>; 2] = [None, None];
    for (i, p) in players.iter().enumerate() {
        if let PlayerKind::Cmd(prog) = p {
            cmds[i] = CmdPlayer::start(prog).ok();
        }
    }
    let mut seconds = [0f64; 2];
    let mut decisions = [0u32; 2];
    let mut events = Vec::new();
    let mut forfeit: Option<usize> = None;
    while !pos.done() {
        if pos.is_chance() {
            let (outs, n) = pos.outcomes();
            let w: Vec<u128> = outs[..n].iter().map(|o| o.1).collect();
            let o = outs[chance.pick_weighted(&w)].0;
            events.push(format!("c{o}"));
            pos = pos.apply_outcome(o);
            continue;
        }
        // Player index 0 is A, 1 is B.
        let who = if pos.mover == seat { 0 } else { 1 };
        let dseed = seed.wrapping_add((pos.step as u64).wrapping_mul(1009)).wrapping_add(3);
        let mut rng = Rng::seed(dseed);
        let t0 = Instant::now();
        let legal = pos.legal();
        let action = match players[who] {
            PlayerKind::Bot(set) => Some(decide(nets, set, &pos, &mut rng).action),
            PlayerKind::Random => {
                let l = legal.as_slice();
                Some(if l.len() == 1 { l[0] } else { l[rng.below(l.len() as u64) as usize] })
            }
            PlayerKind::Cmd(_) => cmds[who].as_mut().and_then(|c| c.ask(&pos, dseed)),
        };
        seconds[who] += t0.elapsed().as_secs_f64();
        decisions[who] += 1;
        match action {
            Some(a) if legal.as_slice().contains(&a) => {
                events.push(format!("a{a}"));
                pos = pos.apply(a);
            }
            _ => {
                forfeit = Some(who);
                break;
            }
        }
    }
    let winner = match (forfeit, pos.winner()) {
        (Some(0), _) => 'B',
        (Some(_), _) => 'A',
        (None, Some(w)) => {
            if w == seat {
                'A'
            } else {
                'B'
            }
        }
        (None, None) => '-',
    };
    GameRecord {
        pair,
        seat,
        winner,
        turns: pos.st.turn_number.saturating_sub(1),
        steps: pos.step,
        seconds,
        decisions,
        events,
        board_pos: start,
    }
}
