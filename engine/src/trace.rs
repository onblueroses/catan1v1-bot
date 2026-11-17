//! Encoding-v1 JSONL: strict replay, constructor and snapshot traces.
//!
//! A trace file holds one game: a board line, a config line, an initial
//! line, one line per step with the state hash after it, and a result line
//! (ENCODING.md, "JSONL traces"). A constructor trace starts from
//! `State::new`. A snapshot trace starts from canonical bytes in `state_hex`
//! and proves nothing about how that state was reached. Replaying a trace
//! written by the other implementation, with equal hashes at every step, is
//! the cross-implementation check of SPEC, "Correctness evidence required",
//! item 4.
use crate::RandomSource;
use crate::runner::{Choice, choose};
use crate::{Board, Config, State, phase};
use serde::{
    Deserialize, Deserializer,
    de::{Error as _, MapAccess, Visitor},
};
use serde_json::{Value, json};
use std::io::{BufRead, Write};

// JSON object keys must be unambiguous. Value alone would silently retain the
// last duplicate, which can hide a conflicting hash, action, or config field.
struct Record(Value);
impl<'de> Deserialize<'de> for Record {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;
        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = Record;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a JSON object with unique keys")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Record, M::Error> {
                let mut object = serde_json::Map::new();
                while let Some((key, value)) = map.next_entry::<String, Value>()? {
                    if object.insert(key.clone(), value).is_some() {
                        return Err(M::Error::custom(format!("Duplicate record field {key}")));
                    }
                }
                Ok(Record(Value::Object(object)))
            }
        }
        deserializer.deserialize_map(ObjectVisitor)
    }
}

/// Lowercase hexadecimal, two digits per byte.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
/// Decode exactly `length` bytes. Uppercase digits are rejected, because the
/// trace format fixes lowercase.
pub fn decode_hex(text: &str, length: usize) -> Result<Vec<u8>, String> {
    if text.len() != length * 2
        || !text
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(format!(
            "Expected exactly {} lowercase hexadecimal digits",
            length * 2
        ));
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).map_err(|e| e.to_string())
        })
        .collect()
}
fn hash(state: &State) -> String {
    format!("{:016x}", state.hash())
}
fn emit(writer: &mut impl Write, value: Value) -> Result<(), String> {
    serde_json::to_writer(&mut *writer, &value).map_err(|e| e.to_string())?;
    writer.write_all(b"\n").map_err(|e| e.to_string())
}

/// Write the board, config and initial lines. With `snapshot` the initial
/// line carries the canonical bytes. Without it the state must be the
/// constructor state, and the call fails otherwise.
pub fn write_header(writer: &mut impl Write, state: &State, snapshot: bool) -> Result<(), String> {
    emit(
        writer,
        json!({"type":"board", "encoding":1, "terrain":state.board.terrain,
        "numbers":state.board.number,"ports":state.board.port_type}),
    )?;
    let mut config = serde_json::to_value(state.config).map_err(|e| e.to_string())?;
    config
        .as_object_mut()
        .ok_or("Config is not an object")?
        .insert("type".into(), json!("config"));
    emit(writer, config)?;
    let mut initial = json!({"type":"initial", "hash":hash(state)});
    if snapshot {
        initial["state_hex"] = json!(hex(&state.canonical_bytes()));
    } else {
        let fresh = State::new(state.board, state.config).map_err(|e| format!("{e:?}"))?;
        if fresh.canonical_bytes() != state.canonical_bytes() {
            return Err("Constructor trace initial state is not fresh".into());
        }
    }
    emit(writer, initial)
}
/// Write one step line. `after` is the state that the choice produced.
pub fn write_step(
    writer: &mut impl Write,
    index: u64,
    choice: Choice,
    after: &State,
) -> Result<(), String> {
    let mut line = json!({"type":"step", "index":index.to_string(), "hash":hash(after)});
    match choice {
        Choice::Action(id) => line["action"] = json!(id),
        Choice::Outcome(id) => line["outcome"] = json!(id),
    }
    emit(writer, line)
}
/// Write the result line: `winner` for a terminal state, otherwise
/// `unfinished`, with reason `dead` when the state is dead (E10).
pub fn write_result(writer: &mut impl Write, steps: u64, state: &State) -> Result<(), String> {
    let terminal = state.phase == phase::TERMINAL;
    let mut result = json!({"type":"result", "status":if terminal {"winner"} else {"unfinished"},
        "winner":if terminal {Some(state.winner)} else {None}, "steps":steps.to_string(),
        "turns":state.turn_number.to_string(), "hash":hash(state)});
    if state.is_dead() {
        result["reason"] = json!("dead");
    }
    emit(writer, result)
}

/// `prefix_steps` is an explicit caller-requested prefix, never a game rule.
pub fn generate(
    writer: &mut impl Write,
    mut state: State,
    rng: &mut impl RandomSource,
    snapshot: bool,
    prefix_steps: Option<u64>,
) -> Result<State, String> {
    write_header(writer, &state, snapshot)?;
    let mut steps = 0u64;
    while !state.is_dead() && prefix_steps.is_none_or(|n| steps < n) {
        let Some(choice) = choose(&state, rng) else {
            break;
        };
        state = choice
            .apply(&state)
            .map_err(|e| format!("{}: {e:?}", context(&state, Some(choice))))?;
        steps = steps.checked_add(1).ok_or("Step count exceeds u64")?;
        write_step(writer, steps, choice, &state)?;
    }
    write_result(writer, steps, &state)?;
    Ok(state)
}

fn field<'a>(v: &'a Value, name: &str) -> Result<&'a Value, String> {
    v.get(name)
        .ok_or_else(|| format!("Missing required field {name}"))
}
fn string<'a>(v: &'a Value, name: &str) -> Result<&'a str, String> {
    field(v, name)?
        .as_str()
        .ok_or_else(|| format!("{name} must be a string"))
}
fn integer(v: &Value, name: &str) -> Result<u64, String> {
    field(v, name)?
        .as_u64()
        .ok_or_else(|| format!("{name} must be an unsigned integer"))
}
fn count(v: &Value, name: &str) -> Result<u64, String> {
    let s = string(v, name)?;
    if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0'))
    {
        return Err(format!(
            "{name} must be a canonical unsigned decimal string"
        ));
    }
    s.parse().map_err(|_| format!("{name} exceeds u64"))
}
fn array<const N: usize>(v: &Value, name: &str) -> Result<[u8; N], String> {
    let values: Vec<u8> =
        serde_json::from_value(field(v, name)?.clone()).map_err(|e| format!("{name}: {e}"))?;
    values
        .try_into()
        .map_err(|_| format!("{name} requires exactly {N} bytes"))
}
fn check_hash(v: &Value, state: &State) -> Result<(), String> {
    let expected = string(v, "hash")?;
    decode_hex(expected, 8)?;
    let actual = hash(state);
    if expected != actual {
        return Err(format!(
            "Hash mismatch: trace {expected}, engine {actual}; {}",
            context(state, None)
        ));
    }
    Ok(())
}
/// Text for an error message: where the replay stood and which SPEC rules
/// govern that phase, so that a mismatch points at the rule family to check.
pub fn context(state: &State, choice: Option<Choice>) -> String {
    let rule = match state.phase {
        0 | 1 => "S1-S4 setup and distance",
        2 | 3 => "T1-T3/C1-C3 roll and dice",
        4 => "T5 discard",
        5 | 6 => "R1-R4 robber and steal",
        7 => "T6/U1-U5/X1-X2/D1-D7 main turn",
        8 => "D1-D2/D7 development draw",
        9 => "D5 road building",
        10 => "D6 year of plenty",
        11 => "D6 monopoly",
        12 => "X1-X2 bank trade",
        13 => "A4 terminal",
        _ => "invalid phase",
    };
    format!(
        "phase={} actor={:?} turn_owner={} turn={} choice={choice:?} rules={rule}; A1-A4 award/win normalization",
        state.phase,
        state.actor(),
        state.turn_owner,
        state.turn_number
    )
}

/// What a successful replay found: the final state and the step counts.
#[derive(Debug)]
pub struct ReplayReport {
    pub state: State,
    pub steps: u64,
    pub decisions: u64,
    pub chance_steps: u64,
    pub snapshot: bool,
}

/// Callback sees each validated state and the next chosen transition, allowing
/// benchmark corpus collection without timing JSON parsing or trace validation.
pub fn replay_with(
    reader: impl BufRead,
    mut visit: impl FnMut(&State, Choice),
) -> Result<ReplayReport, String> {
    let mut board = None;
    let mut config = None;
    let mut state: Option<State> = None;
    let mut snapshot = false;
    let mut steps = 0u64;
    let mut decisions = 0;
    let mut chance_steps = 0;
    let mut finished = false;
    for (line_index, line) in reader.lines().enumerate() {
        let result = (|| -> Result<(), String> {
            if finished {
                return Err("Unexpected record after result".into());
            }
            let line = line.map_err(|e| e.to_string())?;
            let Record(value) =
                serde_json::from_str(&line).map_err(|e| format!("Invalid JSON: {e}"))?;
            if !value.is_object() {
                return Err("Record must be a JSON object".into());
            }
            let kind = string(&value, "type")?;
            match line_index {
                0 => {
                    if kind != "board" || integer(&value, "encoding")? != 1 {
                        return Err("Expected encoding-v1 board record".into());
                    }
                    let b = Board {
                        terrain: array(&value, "terrain")?,
                        number: array(&value, "numbers")?,
                        port_type: array(&value, "ports")?,
                        port_edges: Board::default_port_edges(),
                    };
                    b.validate().map_err(|e| format!("Board invalid: {e:?}"))?;
                    board = Some(b);
                }
                1 => {
                    if kind != "config" {
                        return Err("Expected config record".into());
                    }
                    config = Some(
                        serde_json::from_value::<Config>(value)
                            .map_err(|e| format!("Config: {e}"))?,
                    );
                }
                2 => {
                    if kind != "initial" {
                        return Err("Expected initial record".into());
                    }
                    let fresh = State::new(board.unwrap(), config.unwrap())
                        .map_err(|e| format!("Initial state: {e:?}"))?;
                    let initial = if value.get("state_hex").is_some() {
                        snapshot = true;
                        let bytes = decode_hex(string(&value, "state_hex")?, 313)?;
                        let decoded = State::from_canonical_bytes(&bytes)
                            .map_err(|e| format!("Snapshot invalid: {e:?}"))?;
                        if decoded.canonical_bytes()[..71] != fresh.canonical_bytes()[..71] {
                            return Err("Snapshot board/config differs from header".into());
                        }
                        decoded
                    } else {
                        fresh
                    };
                    check_hash(&value, &initial)?;
                    state = Some(initial);
                }
                _ => {
                    let current = state.as_mut().ok_or("Missing initial state")?;
                    match kind {
                        "step" => {
                            let index = steps.checked_add(1).ok_or("Step count exceeds u64")?;
                            if count(&value, "index")? != index {
                                return Err(format!("Expected step index {index}"));
                            }
                            let action = value.get("action").is_some();
                            let outcome = value.get("outcome").is_some();
                            if action == outcome {
                                return Err("Step requires exactly one of action or outcome".into());
                            }
                            let name = if action { "action" } else { "outcome" };
                            let id = u16::try_from(integer(&value, name)?)
                                .map_err(|_| format!("{name} exceeds u16"))?;
                            let choice = if action {
                                Choice::Action(id)
                            } else {
                                Choice::Outcome(id)
                            };
                            let next = choice.apply(current).map_err(|e| {
                                format!(
                                    "Rejected transition: {e:?}; {}",
                                    context(current, Some(choice))
                                )
                            })?;
                            check_hash(&value, &next).map_err(|e| {
                                format!("{e}; before: {}", context(current, Some(choice)))
                            })?;
                            visit(current, choice);
                            *current = next;
                            steps = index;
                            if action {
                                decisions += 1;
                            } else {
                                chance_steps += 1;
                            }
                        }
                        "result" => {
                            check_hash(&value, current)?;
                            if count(&value, "steps")? != steps
                                || count(&value, "turns")? != current.turn_number
                            {
                                return Err("Result steps/turns differ from replay".into());
                            }
                            let winner = field(&value, "winner")?;
                            match string(&value, "status")? {
                                "winner"
                                    if current.phase == phase::TERMINAL
                                        && winner.as_u64() == Some(u64::from(current.winner)) =>
                                {
                                    ()
                                }
                                "unfinished"
                                    if current.phase != phase::TERMINAL && winner.is_null() =>
                                {
                                    ()
                                }
                                _ => {
                                    return Err(
                                        "Result status/winner disagrees with terminal state".into(),
                                    );
                                }
                            }
                            if let Some(reason) = value.get("reason") {
                                if reason.as_str() != Some("dead") {
                                    return Err("Result reason must be dead when present".into());
                                }
                                if current.phase == phase::TERMINAL || !current.is_dead() {
                                    return Err(
                                        "Result reason dead requires an unfinished dead state"
                                            .into(),
                                    );
                                }
                            }
                            finished = true;
                        }
                        _ => return Err("Expected step or result record".into()),
                    }
                }
            }
            Ok(())
        })();
        result.map_err(|e| format!("Line {}: {e}", line_index + 1))?;
    }
    if !finished {
        return Err("Missing result record".into());
    }
    Ok(ReplayReport {
        state: state.unwrap(),
        steps,
        decisions,
        chance_steps,
        snapshot,
    })
}
/// Replay and check one trace. Any deviation from the format, an illegal
/// transition or a hash mismatch is an error that names the line.
pub fn replay(reader: impl BufRead) -> Result<ReplayReport, String> {
    replay_with(reader, |_, _| {})
}
