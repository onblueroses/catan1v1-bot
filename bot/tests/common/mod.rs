//! Fixture loading shared by the fixture tests.
#![allow(dead_code)]

use catan_bot::position::{Pos, Protocol};
use serde_json::Value;
use std::path::PathBuf;

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The fixture directory; `CATAN_FIXTURES` overrides it (used to show that a corrupted fixture fails).
pub fn fixtures() -> PathBuf {
    std::env::var_os("CATAN_FIXTURES").map(PathBuf::from).unwrap_or_else(|| root().join("tests/fixtures"))
}

pub fn read_jsonl(name: &str) -> Vec<Value> {
    let text = std::fs::read_to_string(fixtures().join(name)).expect(name);
    text.lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str(l).unwrap()).collect()
}

pub fn protocol(v: &Value) -> Protocol {
    serde_json::from_value(v.clone()).unwrap()
}

pub fn pos_of(v: &Value) -> Pos {
    Pos::from_parts(v["state"].as_str().unwrap(), &protocol(&v["protocol"])).unwrap()
}

pub fn bits(v: &Value) -> u32 {
    u32::from_str_radix(v.as_str().unwrap(), 16).unwrap()
}

pub fn nets() -> catan_bot::net::Nets {
    catan_bot::net::Nets::load(&root().join("nets")).unwrap()
}
