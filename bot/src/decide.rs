//! The root procedure (BOT-SPEC 5.1, 7.5) and the public decision entry point.

use crate::board::BoardInfo;
use crate::inputs::{Hidden, viewpoint};
use crate::net::Nets;
use crate::position::{Pos, bp};
use crate::rng::Rng;
use crate::rollout::{RolloutCtx, TraceEvent, rollout_stage};
use crate::search::{Node, Search, Settings, chance_mean, rank_desc};
use crate::value::{Mode, Valuer};

#[derive(Clone, Debug)]
pub struct Decision {
    pub action: u8,
    pub value: f32,
    /// Root actions with their values, in the order the root valued them.
    pub values: Vec<(u8, f32)>,
}

/// Decide for the position's mover.
pub fn decide(nets: &Nets, set: &Settings, pos: &Pos, rng: &mut Rng) -> Decision {
    decide_traced(nets, set, pos, rng, None)
}

pub fn decide_traced(
    nets: &Nets,
    set: &Settings,
    pos: &Pos,
    rng: &mut Rng,
    trace: Option<&mut Vec<TraceEvent>>,
) -> Decision {
    let bi = BoardInfo::new(&pos.st);
    let v = pos.mover;
    let (vp, hid) = viewpoint(pos, v);
    let mode = if pos.phase <= bp::SETUP_ROAD { Mode::Opening } else { Mode::Main };
    decide_view(nets, &bi, set, &Node { pos: vp, d: hid.d }, v, hid, mode, rng, trace)
}

/// The root procedure on a viewpoint node.
#[allow(clippy::too_many_arguments)]
pub fn decide_view(
    nets: &Nets,
    bi: &BoardInfo,
    set: &Settings,
    root: &Node,
    v: u8,
    hid: Hidden,
    mode: Mode,
    rng: &mut Rng,
    trace: Option<&mut Vec<TraceEvent>>,
) -> Decision {
    let val = Valuer { nets, bi, mode, v, h: hid.h, n: hid.n, econ: set.econ, card: set.card };
    let s = Search::new(val, *set, root.d);
    let acts_list = s.legal(root);
    let acts = acts_list.as_slice();
    if acts.len() == 1 {
        return Decision { action: acts[0], value: -1.0, values: Vec::new() };
    }
    assert!(!acts.is_empty(), "no legal action at the root");

    // values[i] belongs to acts[i]; `order` is the order of valuation.
    let mut values: Vec<f32>;
    let mut order: Vec<usize> = (0..acts.len()).collect();
    let mut best: f32;

    if mode == Mode::Opening {
        let kids: Vec<Node> = acts.iter().map(|&a| root.child(a)).collect();
        let searched: Vec<usize>;
        if root.phase() == bp::SETUP_SETTLEMENT {
            let cv: Vec<f32> = kids.iter().map(|c| s.value(c)).collect();
            order = rank_desc(&cv);
            searched = order.clone();
        } else if acts.len() > set.opening_width {
            let cv: Vec<f32> = kids.iter().map(|c| s.value(c)).collect();
            let mut sorted = cv.clone();
            sorted.sort_by(|a, b| b.partial_cmp(a).unwrap());
            let cut = sorted[set.opening_width - 1];
            searched = (0..acts.len()).filter(|&i| cv[i] >= cut).collect();
        } else {
            searched = (0..acts.len()).collect();
        }
        values = vec![-1.0; acts.len()];
        best = -1.0;
        for &i in &searched {
            let low = best;
            let x = s.node_value(&kids[i], low, 2.0);
            values[i] = if x <= low { -1.0 } else { x };
            best = best.max(values[i]);
        }
    } else {
        if set.pre_roll && root.pos.before_roll && (root.phase() == bp::ROBBER || root.phase() == bp::ROAD_BUILDING) {
            let (a, x) = s.pre_step(root);
            rng.below(1);
            return Decision { action: a.unwrap(), value: x, values: Vec::new() };
        }
        if root.phase() == bp::MAIN {
            values = main_values(&s, root, acts, hid, trace);
            best = values.iter().cloned().fold(-2.0f32, f32::max);
        } else {
            values = vec![0.0; acts.len()];
            best = -1.0;
            for (i, &a) in acts.iter().enumerate() {
                let c = root.child(a);
                values[i] = if set.pre_roll && root.phase() == bp::PRE_ROLL {
                    s.finish_before_roll(&c)
                } else {
                    s.after(&c, set.one_step_depth - 1)
                };
                best = best.max(values[i]);
            }
        }
        // Robber recheck.
        if set.robber_rechecks > 0
            && root.phase() == bp::ROBBER
            && !root.pos.before_roll
            && root.pos.st.turn_owner == v
        {
            let ranked = rank_desc(&values);
            let mut nv = vec![-1.0f32; acts.len()];
            best = -1.0;
            for &i in ranked.iter().take(set.robber_rechecks) {
                let c = root.child(acts[i]);
                nv[i] = if c.is_chance() {
                    let (kids, k) = c.chance_children();
                    chance_mean(kids[..k].iter().map(|(cc, w)| (plan_value(&s, cc), *w)))
                } else {
                    plan_value(&s, &c)
                };
                best = best.max(nv[i]);
            }
            values = nv;
        }
    }

    let cands: Vec<usize> = (0..acts.len()).filter(|&i| values[i] >= best).collect();
    let pick = cands[rng.below(cands.len() as u64) as usize];
    Decision {
        action: acts[pick],
        value: best,
        values: order.iter().map(|&i| (acts[i], values[i])).collect(),
    }
}

/// plan(s) of the robber recheck: a fresh plan search without reply or rollouts.
fn plan_value(s: &Search, n: &Node) -> f32 {
    if n.done() || n.phase() != bp::MAIN || n.mover() != s.v() {
        return s.leaf(n);
    }
    let acts = s.legal(n);
    let (vals, _, _) = s.beam(n, acts.as_slice());
    vals.iter().cloned().fold(-2.0f32, f32::max)
}

/// Plan search, reply check and rollouts at the viewer's own main phase.
fn main_values(s: &Search, root: &Node, acts: &[u8], hid: Hidden, trace: Option<&mut Vec<TraceEvent>>) -> Vec<f32> {
    let (mut values, endpoints, won) = s.beam(root, acts);
    if won {
        return values;
    }
    let set = &s.set;
    if set.reply_finalists > 0 {
        let have: Vec<usize> = (0..acts.len()).filter(|&i| endpoints[i].is_some()).collect();
        let mut list: Vec<usize> = have.clone();
        list.sort_by(|&a, &b| values[b].partial_cmp(&values[a]).unwrap().then(a.cmp(&b)));
        if set.rollout_samples > 0 {
            let mut keys: Vec<[u8; 181]> = Vec::new();
            list.retain(|&i| {
                let k = end_key(s, endpoints[i].as_ref().unwrap());
                if keys.contains(&k) {
                    false
                } else {
                    keys.push(k);
                    true
                }
            });
        }
        values.iter_mut().for_each(|x| *x = -2.0);
        for &i in list.iter().take(set.reply_finalists) {
            values[i] = s.finish_turn(endpoints[i].as_ref().unwrap());
        }
    }
    if set.rollout_samples > 0 {
        let ctx = RolloutCtx { nets: s.val.nets, bi: s.val.bi, set: *set, v: s.v(), hid };
        rollout_stage(&ctx, root, acts, &mut values, &endpoints, trace);
    }
    values
}

/// End key of an endpoint: the 181-byte key after 181 at an unfinished main phase, with the root D.
pub fn end_key(s: &Search, e: &Node) -> [u8; 181] {
    let n = if e.phase() == bp::MAIN && !e.done() { e.child(crate::position::END_TURN) } else { *e };
    crate::keys::key181(&n.pos, s.root_d)
}
