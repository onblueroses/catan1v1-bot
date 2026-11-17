//! The search value of a viewpoint (BOT-SPEC 4.4).

use crate::board::BoardInfo;
use crate::geom::{geometry, ways};
use crate::inputs::{Hidden, Inputs, N_INPUTS, Pieces, compute_inputs};
use crate::net::Nets;
use crate::position::{Pos, bp};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Opening,
    Main,
}

const WR: [f64; 5] = [1.0, 1.0, 1.0, 1.5, 2.0];

/// Everything a search value depends on besides the node: the nets, the board,
/// the mode, the root viewer and its H and N, and the two term weights.
#[derive(Clone, Copy)]
pub struct Valuer<'a> {
    pub nets: &'a Nets,
    pub bi: &'a BoardInfo,
    pub mode: Mode,
    pub v: u8,
    pub h: u8,
    pub n: u8,
    pub econ: f32,
    pub card: f32,
}

/// Production log-score of player w (economy term).
fn production_score(pos: &Pos, pc: &Pieces, bi: &BoardInfo, w: usize) -> f64 {
    let g = geometry();
    let robber = g.tile_of_engine[pos.st.robber_hex as usize] as usize;
    let mut income = [0f64; 5];
    for t in 0..19 {
        let res = bi.res[t];
        if res == 5 || t == robber {
            continue;
        }
        let c = pc.weight(w, t);
        income[res as usize] += (c as f64 * ways(bi.num[t]) as f64) / 36.0;
    }
    let mut s = 0.0f64;
    for r in 0..5 {
        s += WR[r] * (4.0 * income[r]).ln_1p();
    }
    s
}

#[inline]
fn logit_clamped(p: f32) -> f32 {
    let q = p.clamp(0.00001f32, 0.99999f32);
    (q / (1.0f32 - q)).ln()
}

impl<'a> Valuer<'a> {
    pub fn hidden(&self, d: u8) -> Hidden {
        Hidden { h: self.h, n: self.n, d }
    }

    pub fn inputs(&self, pos: &Pos, d: u8) -> Inputs {
        let mut x = [0u8; N_INPUTS];
        compute_inputs(pos, self.v, self.hidden(d), self.bi, &mut x);
        x
    }

    /// The raw network value p of step 3.
    pub fn raw(&self, pos: &Pos, x: &Inputs) -> f32 {
        match self.mode {
            Mode::Opening if pos.phase >= bp::PRE_ROLL => self.nets.main.eval_float(x),
            Mode::Opening => (self.nets.opening.eval_fixed(x) + self.nets.main.eval_fixed(x)) / 2.0f32,
            Mode::Main => self.nets.main.eval_fixed(x),
        }
    }

    /// Steps 4-6: the economy and card terms on top of a raw value.
    pub fn terms(&self, pos: &Pos, mut p: f32) -> f32 {
        if self.econ != 0.0 {
            let pc = Pieces::of(pos);
            let v = self.v as usize;
            let pv = production_score(pos, &pc, self.bi, v);
            let pr = production_score(pos, &pc, self.bi, 1 - v);
            let l = logit_clamped(p) as f64;
            p = (1.0f64 / (1.0f64 + (-l - self.econ as f64 * (pv - pr)).exp())) as f32;
        }
        if self.card != 0.0 {
            let v = self.v as usize;
            let k: i32 = (0..5)
                .map(|t| (pos.st.playable_development[v][t] + pos.st.new_development[v][t]) as i32)
                .sum();
            let l = logit_clamped(p) + self.card * (k - self.h as i32) as f32;
            p = (1.0f64 / (1.0f64 + (-(l as f64)).exp())) as f32;
        }
        p
    }

    /// The search value of the viewpoint node (`pos` hides the rival's cards, deck count d).
    pub fn value(&self, pos: &Pos, d: u8) -> f32 {
        if let Some(w) = pos.winner() {
            return if w == self.v { 1.0 } else { 0.0 };
        }
        let x = self.inputs(pos, d);
        let p = self.raw(pos, &x);
        self.terms(pos, p)
    }

    /// The leaf value (main search): clamped unless terminal.
    pub fn leaf_main(&self, pos: &Pos, d: u8) -> f32 {
        let v = self.value(pos, d);
        if pos.is_terminal() { v } else { v.clamp(0.000001f32, 0.999999f32) }
    }
}
