//! Micro-benchmark: inputs, fixed eval, state copy+apply, compatibility bytes.
use catan_bot::board::BoardInfo;
use catan_bot::inputs::{compute_inputs, viewpoint};
use catan_bot::net::Nets;
use catan_bot::position::{Pos, Protocol};
use std::time::Instant;
fn main() {
    if let Some(path) = std::env::args().nth(1) {
        return replay(&path);
    }
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let nets = Nets::load(&root.join("nets")).unwrap();
    let ps: Vec<Pos> = std::fs::read_to_string(root.join("tests/fixtures/positions.jsonl")).unwrap().lines().map(|l| {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        let pr: Protocol = serde_json::from_value(v["protocol"].clone()).unwrap();
        Pos::from_parts(v["state"].as_str().unwrap(), &pr).unwrap()
    }).filter(|p| p.phase == 7).collect();
    let bis: Vec<BoardInfo> = ps.iter().map(|p| BoardInfo::new(&p.st)).collect();
    let mut xs = Vec::new();
    let t = Instant::now();
    for _ in 0..10 { xs.clear(); for (p, bi) in ps.iter().zip(&bis) { let (vp, h) = viewpoint(p, p.mover); let mut x = [0u8; 351]; compute_inputs(&vp, p.mover, h, bi, &mut x); xs.push(x); } }
    let n = (ps.len() * 10) as f64;
    println!("inputs+viewpoint: {:.2} us", t.elapsed().as_secs_f64() / n * 1e6);
    let t = Instant::now(); let mut s = 0f32;
    for _ in 0..10 { for x in &xs { s += nets.main.eval_fixed(x); } }
    println!("eval_fixed: {:.2} us ({s})", t.elapsed().as_secs_f64() / n * 1e6);
    let t = Instant::now(); let mut k = 0usize;
    for _ in 0..10 { for p in &ps { k += p.st.compatibility_bytes()[100] as usize; } }
    println!("compat bytes: {:.2} us ({k})", t.elapsed().as_secs_f64() / n * 1e6);
    let t = Instant::now(); let mut m = 0usize;
    for _ in 0..10 { for p in &ps { let l = p.legal(); m += l.len(); for &a in l.as_slice() { let c = p.apply(a); m += c.step as usize; } } }
    println!("legal + apply all: {:.2} us per position ({m})", t.elapsed().as_secs_f64() / n * 1e6);
}

/// Replay a file of captured 351-byte input vectors through the main net, in order.
fn replay(path: &str) {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let nets = Nets::load(&root.join("nets")).unwrap();
    let b = std::fs::read(path).unwrap();
    let xs: Vec<[u8; 351]> = b.chunks_exact(351).map(|c| c.try_into().unwrap()).collect();
    for _ in 0..2 {
        let t = Instant::now();
        let mut s = 0f64;
        for x in &xs {
            s += nets.main.eval_fixed(x) as f64;
        }
        println!("replay {} vectors: {:.3} us each (checksum {s:.9})", xs.len(), t.elapsed().as_secs_f64() / xs.len() as f64 * 1e6);
    }
}
