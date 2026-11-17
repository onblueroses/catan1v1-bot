//! Search inside one decision: settings, search nodes, chance means, the
//! opening alpha-beta with its goal bonus (BOT-SPEC 5), the whole-turn plan
//! search (6), the reply check (7.1-7.3) and the plain one-step helpers (7.4).

use crate::geom::{bits64, geometry, ways, NODE, EDGE};
use crate::gccsort::gcc_sort;
use crate::inputs::{Pieces, fund};
use crate::position::{Acts, BUY_CARD, END_TURN, Pos, ROLL, bp, trade_id};
use crate::value::{Mode, Valuer};
use catan_engine::phase as ep;
use std::cell::RefCell;

/// Bot settings (BOT-SPEC 1, 8.6).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub beam_width: usize,
    pub plan_depth: usize,
    pub opening_width: usize,
    pub goal_reach: u32,
    pub patience: f32,
    pub econ: f32,
    pub card: f32,
    pub robber_rechecks: usize,
    pub reply_finalists: usize,
    pub rollout_samples: usize,
    pub workers: usize,
    pub one_step_depth: u32,
    pub pre_roll: bool,
}

impl Settings {
    pub fn vinco() -> Settings {
        Settings {
            beam_width: 32,
            plan_depth: 6,
            opening_width: 6,
            goal_reach: 2,
            patience: 0.75,
            econ: 0.25,
            card: 0.1,
            robber_rechecks: 4,
            reply_finalists: 4,
            rollout_samples: 128,
            workers: 4,
            one_step_depth: 2,
            pre_roll: true,
        }
    }
    pub fn reply() -> Settings {
        Settings { rollout_samples: 0, ..Self::vinco() }
    }
    pub fn beam() -> Settings {
        Settings { reply_finalists: 0, rollout_samples: 0, ..Self::vinco() }
    }
    /// The rollout player of BOT-SPEC 8.6.
    pub fn rollout_player() -> Settings {
        Settings {
            beam_width: 2,
            plan_depth: 3,
            econ: 0.0,
            card: 0.1,
            robber_rechecks: 4,
            reply_finalists: 0,
            rollout_samples: 0,
            one_step_depth: 1,
            pre_roll: false,
            ..Self::vinco()
        }
    }
    pub fn preset(name: &str) -> Option<Settings> {
        match name {
            "vinco" => Some(Self::vinco()),
            "reply" => Some(Self::reply()),
            "beam" => Some(Self::beam()),
            _ => None,
        }
    }
}

/// A search node: a viewpoint position and the deck count D that the search follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub pos: Pos,
    pub d: u8,
}

impl Node {
    #[inline]
    pub fn child(&self, a: u8) -> Node {
        Node { pos: self.pos.apply(a), d: self.d }
    }
    #[inline]
    pub fn phase(&self) -> u8 {
        self.pos.phase
    }
    #[inline]
    pub fn mover(&self) -> u8 {
        self.pos.mover
    }
    #[inline]
    pub fn done(&self) -> bool {
        self.pos.done()
    }
    #[inline]
    pub fn is_chance(&self) -> bool {
        self.pos.is_chance()
    }
    /// The chance children with their weights (D - 1 below a draw).
    pub fn chance_children(&self) -> ([(Node, u128); 11], usize) {
        let (outs, n) = self.pos.outcomes();
        let draw = self.pos.st.phase == ep::DEVELOPMENT_DRAW;
        let d = if draw { self.d - 1 } else { self.d };
        let mut out = [(*self, 0u128); 11];
        for i in 0..n {
            out[i] = (Node { pos: self.pos.apply_outcome(outs[i].0), d }, outs[i].1);
        }
        (out, n)
    }
}

/// Chance mean (BOT-SPEC 4.5) over `(value, weight)` terms.
pub fn chance_mean(terms: impl IntoIterator<Item = (f32, u128)>) -> f32 {
    let mut s = 0f64;
    let mut t = 0f64;
    for (v, w) in terms {
        s += ((w as f32) * v) as f64;
        t += w as f64;
    }
    (s / t) as f32
}

/// Sort indices by value descending, ties by index.
pub fn rank_desc(values: &[f32]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..values.len()).collect();
    idx.sort_by(|&a, &b| values[b].partial_cmp(&values[a]).unwrap().then(a.cmp(&b)));
    idx
}

/// "The same partial plan" (BOT-SPEC 6.2).
pub fn same_plan(a: &Pos, b: &Pos) -> bool {
    let (x, y) = (&a.st, &b.st);
    x.node_occupancy == y.node_occupancy
        && x.edge_occupancy == y.edge_occupancy
        && x.resources == y.resources
        && x.bank == y.bank
        && x.playable_development == y.playable_development
        && x.new_development == y.new_development
        && x.spent_development == y.spent_development
        && x.development_played == y.development_played
        && x.discard_remaining == y.discard_remaining
        && x.largest_army_holder == y.largest_army_holder
        && x.longest_road_holder == y.longest_road_holder
        && x.cache.road_lengths == y.cache.road_lengths
        && x.cache.trade_rates == y.cache.trade_rates
        && x.robber_hex == y.robber_hex
        && x.turn_owner == y.turn_owner
        && a.phase == b.phase
        && a.mover == b.mover
        && a.before_roll == b.before_roll
        && a.step == b.step
        && a.free_roads_left == b.free_roads_left
        && a.discard_active == b.discard_active
        && a.discard_slots == b.discard_slots
}

/// Length of a value-cache key (see `value_key`).
const VKEY: usize = 384;

/// Identity of a search value: the state bytes, the cached road lengths (goal
/// states keep stale ones), the protocol, D, and every valuer parameter (nets,
/// mode, viewer, H, N, term weights). The value is a pure function of it.
fn value_key(n: &Node, val: &Valuer) -> [u8; VKEY] {
    let mut k = pos_key(&n.pos, n.d);
    let b = &mut k[323..];
    b[34] = match val.mode {
        Mode::Opening => 1,
        Mode::Main => 2,
    };
    b[35] = val.v;
    b[36] = val.h;
    b[37] = val.n;
    b[38..42].copy_from_slice(&val.econ.to_bits().to_le_bytes());
    b[42..46].copy_from_slice(&val.card.to_bits().to_le_bytes());
    b[46..50].copy_from_slice(&(val.nets.main.id() as u32).to_le_bytes());
    b[50..54].copy_from_slice(&(val.nets.opening.id() as u32).to_le_bytes());
    k
}

/// Key of a rollout stop value (BOT-SPEC 8.5): the position, the root viewer
/// and the main net. Byte 34 = 3 keeps it apart from search values.
pub fn stop_key(r: &Pos, v: u8, main_net: usize) -> [u8; VKEY] {
    let mut k = pos_key(r, 0);
    let b = &mut k[323..];
    b[34] = 3;
    b[35] = v;
    b[46..50].copy_from_slice(&(main_net as u32).to_le_bytes());
    k
}

/// The position part of a cache key: state bytes, protocol, cached road lengths, D.
fn pos_key(p: &Pos, d: u8) -> [u8; VKEY] {
    let mut k = [0u8; VKEY];
    k[..323].copy_from_slice(&p.st.compatibility_bytes());
    let b = &mut k[323..];
    b[0] = p.phase;
    b[1] = p.mover;
    b[2] = p.before_roll as u8;
    b[3] = p.setup_placed;
    b[4] = p.free_roads_left;
    b[5] = p.last_total;
    b[6..10].copy_from_slice(&p.step.to_le_bytes());
    b[10] = p.discard_active;
    b[11] = p.discard_slots[0][0];
    b[12] = p.discard_slots[0][1];
    b[13] = p.discard_slots[1][0];
    b[14] = p.discard_slots[1][1];
    b[15] = p.port_ring;
    b[16..20].copy_from_slice(&p.step_limit.to_le_bytes());
    b[20..31].copy_from_slice(&p.dice_counts);
    b[31] = p.st.cache.road_lengths[0];
    b[32] = p.st.cache.road_lengths[1];
    b[33] = d;
    k
}

/// The cached value of `key`, or `f()` stored under it. Values are pure
/// functions of their keys, so a hit equals a fresh computation.
#[inline]
pub fn cached(key: &[u8; VKEY], f: impl FnOnce() -> f32) -> f32 {
    let h = key_hash(key) | 1;
    let slot = (h >> (64 - VCACHE_BITS)) as usize;
    let hit = VCACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.keys.is_empty() {
            c.tags = vec![0u64; 1 << VCACHE_BITS];
            c.keys = vec![[0u8; VKEY]; 1 << VCACHE_BITS];
            c.vals = vec![0f32; 1 << VCACHE_BITS];
        }
        if c.tags[slot] == h && key_eq(&c.keys[slot], key) { Some(c.vals[slot]) } else { None }
    });
    if let Some(v) = hit {
        return v;
    }
    let v = f();
    VCACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.tags[slot] = h;
        c.keys[slot] = *key;
        c.vals[slot] = v;
    });
    v
}

/// Key equality without a library call: XOR of all words, then one test.
#[inline]
fn key_eq(a: &[u8; VKEY], b: &[u8; VKEY]) -> bool {
    let mut d = 0u64;
    for i in 0..VKEY / 8 {
        let x = u64::from_le_bytes(a[8 * i..8 * i + 8].try_into().unwrap());
        let y = u64::from_le_bytes(b[8 * i..8 * i + 8].try_into().unwrap());
        d |= x ^ y;
    }
    d == 0
}

#[inline]
fn key_hash(k: &[u8; VKEY]) -> u64 {
    const M: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut h = [0x243f_6a88_85a3_08d3u64, 0x1319_8a2e_0370_7344, 0xa409_3822_299f_31d0, 0x082e_fa98_ec4e_6c89];
    for (i, c) in k.chunks_exact(8).enumerate() {
        let w = u64::from_le_bytes(c.try_into().unwrap());
        let l = i & 3;
        h[l] = (h[l] ^ w).wrapping_mul(M).rotate_left(29);
    }
    let mut x = h[0] ^ h[1].rotate_left(16) ^ h[2].rotate_left(32) ^ h[3].rotate_left(48);
    x ^= x >> 31;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^ (x >> 29)
}

/// log2 of the value-cache entries per thread.
const VCACHE_BITS: u32 = VCB;
const VCB: u32 = 16;

/// A per-thread direct-mapped cache of search values. Every lookup compares
/// the whole key, so a hit returns the value that a fresh computation gives;
/// an evicted entry is computed again.
struct ValueCache {
    /// Hash of the stored key (0 for an empty slot), checked before the key so
    /// that most misses do not load the key.
    tags: Vec<u64>,
    keys: Vec<[u8; VKEY]>,
    vals: Vec<f32>,
}

thread_local! {
    static VCACHE: RefCell<ValueCache> = const { RefCell::new(ValueCache { tags: Vec::new(), keys: Vec::new(), vals: Vec::new() }) };
}

/// One search: the valuer (mode, viewer, H, N, weights), the settings and the root D.
pub struct Search<'a> {
    pub val: Valuer<'a>,
    pub set: Settings,
    pub root_d: u8,
}

impl<'a> Search<'a> {
    pub fn new(val: Valuer<'a>, set: Settings, root_d: u8) -> Search<'a> {
        Search { val, set, root_d }
    }

    #[inline]
    pub fn v(&self) -> u8 {
        self.val.v
    }

    /// Legal actions in the search: minus 182 when D = 0.
    pub fn legal(&self, n: &Node) -> Acts {
        let mut a = n.pos.legal();
        if n.d == 0 {
            a.remove(BUY_CARD);
        }
        a
    }

    /// The search value (4.4) of a node.
    pub fn value(&self, n: &Node) -> f32 {
        if let Some(w) = n.pos.winner() {
            return if w == self.val.v { 1.0 } else { 0.0 };
        }
        cached(&value_key(n, &self.val), || self.val.value(&n.pos, n.d))
    }

    /// The leaf value.
    pub fn leaf(&self, n: &Node) -> f32 {
        match self.val.mode {
            Mode::Main => {
                let v = self.value(n);
                if n.pos.is_terminal() { v } else { v.clamp(0.000001f32, 0.999999f32) }
            }
            Mode::Opening => self.opening_leaf(n),
        }
    }

    pub fn chance_mean_of(&self, n: &Node, mut f: impl FnMut(&Node) -> f32) -> f32 {
        // The children of `chance_children`, made one at a time in the same order.
        let (outs, k) = n.pos.outcomes();
        let d = if n.pos.st.phase == ep::DEVELOPMENT_DRAW { n.d - 1 } else { n.d };
        chance_mean(outs[..k].iter().map(|&(o, w)| (f(&Node { pos: n.pos.apply_outcome(o), d }), w)))
    }

    // ---------------- Opening (5) ----------------

    /// Node value with alpha-beta bounds (5.2).
    pub fn node_value(&self, s: &Node, low: f32, high: f32) -> f32 {
        if s.phase() > bp::SETUP_ROAD {
            return self.opening_leaf(s);
        }
        let side: f32 = if s.mover() == self.v() { 1.0 } else { -1.0 };
        let acts = self.legal(s);
        let kids: Vec<Node> = acts.as_slice().iter().map(|&a| s.child(a)).collect();
        let vals: Vec<f32> = kids.iter().map(|c| side * self.value(c)).collect();
        let order = rank_desc(&vals);
        let width = if s.phase() == bp::SETUP_ROAD { 1 } else { self.set.opening_width };
        let mut best = -2.0f32;
        for &i in order.iter().take(width) {
            let v = if side > 0.0 {
                self.node_value(&kids[i], low.max(best), high)
            } else {
                self.node_value(&kids[i], low, high.min(-best))
            };
            best = best.max(side * v);
            if (side > 0.0 && best >= high) || (side < 0.0 && -best <= low) {
                break;
            }
        }
        side * best
    }

    /// Opening leaf value (5.3).
    pub fn opening_leaf(&self, s: &Node) -> f32 {
        if s.pos.is_terminal() {
            return self.value(s);
        }
        let mut g = self.value(s);
        g = g + self.goal(s, self.v(), g);
        g = g - self.goal(s, 1 - self.v(), g);
        g
    }

    /// Goal bonus (5.4).
    pub fn goal(&self, s: &Node, w: u8, base: f32) -> f32 {
        let gm = geometry();
        let bi = self.val.bi;
        let wu = w as usize;
        let sigma: f32 = if w == self.v() { 1.0 } else { -1.0 };
        let st = &s.pos.st;
        let pc = Pieces::of(&s.pos);
        let robber = gm.tile_of_engine[st.robber_hex as usize] as usize;
        let mut income = [0f64; 5];
        for t in 0..19 {
            let c = pc.weight(wu, t);
            let res = bi.res[t];
            if c == 0 || res == 5 || t == robber {
                continue;
            }
            income[res as usize] += (c as f64 * ways(bi.num[t]) as f64) / 36.0;
        }
        let wait = if s.phase() == bp::MAIN && s.mover() == w { 0 } else { 1 };
        let hand = st.resources[wu];
        let ratio = st.cache.trade_rates[wu];
        let mut best = 0.0f32;
        let mut consider = |b: &mut Pos, cost: [u8; 5]| {
            let turns = fund(&hand, &income, &ratio, &cost).max(wait);
            if turns > 10 {
                return;
            }
            for r in 0..5 {
                b.st.resources[wu][r] = hand[r].saturating_sub(cost[r]);
            }
            b.st.recompute_caches();
            b.st.cache.road_lengths = st.cache.road_lengths;
            let bn = Node { pos: *b, d: s.d };
            let gain = (self.set.patience.powf(turns as f32) * sigma) * (self.value(&bn) - base);
            best = best.max(gain);
        };
        // City goals.
        if pc.city[wu].count_ones() < 4 {
            let mut nodes: Vec<usize> = bits64(pc.settle[wu]).collect();
            nodes.sort_by(|&a, &b| bi.pips[b].cmp(&bi.pips[a]));
            for &n in nodes.iter().take(2) {
                let mut b = s.pos;
                b.st.node_occupancy[NODE[n] as usize] = 3 + w;
                consider(&mut b, [0, 0, 0, 2, 3]);
            }
        }
        // Settlement goals.
        if pc.settle[wu].count_ones() < 5 {
            let walls = pc.built(1 - wu);
            let blocked = pc.blocked();
            let mut f = (pc.built(wu) | pc.ends(wu)) & !walls;
            let mut seen = f;
            let taken = pc.roads[0] | pc.roads[1];
            let mut path = [0u128; 54];
            let mut spots: Vec<(usize, u32)> = bits64(f & !blocked).map(|n| (n, 0)).collect();
            let nroads = pc.roads[wu].count_ones();
            for step in 1..=self.set.goal_reach {
                if nroads + step > 15 {
                    break;
                }
                let mut x = 0u64;
                for n in bits64(f) {
                    for &e in gm.edges_of(n) {
                        let [a, b2] = gm.edge_nodes[e as usize];
                        let m = if a as usize == n { b2 } else { a } as usize;
                        if taken >> e & 1 == 1 || seen >> m & 1 == 1 || x >> m & 1 == 1 {
                            continue;
                        }
                        x |= 1u64 << m;
                        path[m] = path[n] | (1u128 << e);
                    }
                }
                seen |= x;
                f = x & !walls;
                spots.extend(bits64(x & !blocked).map(|n| (n, step)));
            }
            let key = |&(n, r): &(usize, u32)| bi.pips[n] as i64 - 2 * r as i64;
            gcc_sort(&mut spots, |a, b| key(a) > key(b));
            for &(n, roads) in spots.iter().take(3) {
                let mut b = s.pos;
                for e in crate::geom::bits128(path[n]) {
                    b.st.edge_occupancy[EDGE[e] as usize] = 1 + w;
                }
                b.st.node_occupancy[NODE[n] as usize] = 1 + w;
                let r = roads as u8;
                consider(&mut b, [1 + r, 1 + r, 1, 1, 0]);
            }
        }
        best
    }

    // ---------------- Plan search (6) ----------------

    /// Turn-end value E (6.1).
    pub fn turn_end(&self, s: &Node) -> f32 {
        if s.done() || s.mover() != self.v() {
            return self.leaf(s);
        }
        if s.is_chance() {
            return self.chance_mean_of(s, |c| self.turn_end(c));
        }
        match s.phase() {
            bp::MAIN => self.leaf(&s.child(END_TURN)),
            bp::ROBBER | bp::ROAD_BUILDING => {
                let acts = self.legal(s);
                let kids: Vec<Node> = acts.as_slice().iter().map(|&a| s.child(a)).collect();
                let vals: Vec<f32> = kids.iter().map(|c| self.value(c)).collect();
                let order = rank_desc(&vals);
                let take = if s.phase() == bp::ROAD_BUILDING && s.pos.free_roads_left == 2 { 3 } else { order.len() };
                let mut best = 0.0f32;
                for &i in order.iter().take(take) {
                    best = best.max(self.turn_end(&kids[i]));
                }
                best
            }
            _ => self.leaf(s),
        }
    }

    /// Beam search over whole-turn plans (6.2). Returns values (init -2) and endpoints per root action.
    pub fn beam(&self, root: &Node, acts: &[u8]) -> (Vec<f32>, Vec<Option<Node>>, bool) {
        let v = self.v();
        let mut value = vec![-2.0f32; acts.len()];
        let mut endpoint: Vec<Option<Node>> = vec![None; acts.len()];
        let mut frontier: Vec<(Node, usize)> = vec![(*root, usize::MAX)];
        for ply in 0..self.set.plan_depth {
            if frontier.is_empty() {
                break;
            }
            let mut cands: Vec<(Node, usize, f32)> = Vec::new();
            for (line, first) in &frontier {
                let line_acts;
                let list: Vec<(u8, usize)> = if ply == 0 {
                    acts.iter().enumerate().map(|(i, &a)| (a, i)).collect()
                } else {
                    line_acts = self.legal(line);
                    line_acts.as_slice().iter().map(|&a| (a, *first)).collect()
                };
                for (a, f) in list {
                    let next = line.child(a);
                    if next.pos.winner() == Some(v) {
                        // Values found earlier in this search are kept (fixture reading).
                        value[f] = 1.0;
                        endpoint[f] = Some(next);
                        return (value, endpoint, true);
                    }
                    if next.done() || next.is_chance() || next.mover() != v {
                        let x = self.turn_end(&next);
                        if x > value[f] {
                            value[f] = x;
                            endpoint[f] = Some(next);
                        }
                    } else {
                        let rank = if next.phase() == bp::MAIN {
                            self.value(&next.child(END_TURN))
                        } else {
                            self.value(&next)
                        };
                        cands.push((next, f, rank));
                    }
                }
            }
            cands.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
            let mut kept: Vec<(Node, usize)> = Vec::new();
            for (n, f, _) in cands {
                if kept.len() >= self.set.beam_width {
                    break;
                }
                if !kept.iter().any(|(k, _)| same_plan(&k.pos, &n.pos)) {
                    kept.push((n, f));
                }
            }
            frontier = kept;
        }
        for (line, f) in &frontier {
            if *f == usize::MAX {
                continue;
            }
            let x = self.turn_end(line);
            if x > value[*f] {
                value[*f] = x;
                endpoint[*f] = Some(*line);
            }
        }
        (value, endpoint, false)
    }

    // ---------------- Reply check (7) ----------------

    /// W: finish the viewer's turn, then the rival's public reply (7.2).
    pub fn finish_turn(&self, s: &Node) -> f32 {
        if s.done() || s.mover() != self.v() {
            return self.reply_value(s);
        }
        if s.is_chance() {
            return self.chance_mean_of(s, |c| self.finish_turn(c));
        }
        match s.phase() {
            bp::MAIN => self.reply_value(&s.child(END_TURN)),
            bp::ROBBER | bp::ROAD_BUILDING => {
                let acts = self.legal(s);
                if acts.is_empty() {
                    return self.leaf(s);
                }
                let kids: Vec<Node> = acts.as_slice().iter().map(|&a| s.child(a)).collect();
                let vals: Vec<f32> = kids.iter().map(|c| self.turn_end(c)).collect();
                let order = rank_desc(&vals);
                self.finish_turn(&kids[order[0]])
            }
            _ => self.leaf(s),
        }
    }

    /// Q: the rival's public reply (7.2).
    pub fn reply_value(&self, s: &Node) -> f32 {
        let v = self.v();
        if s.done() || (s.phase() == bp::PRE_ROLL && s.mover() == v) {
            return self.leaf(s);
        }
        if s.phase() == bp::PRE_ROLL {
            return self.reply_value(&s.child(ROLL));
        }
        if s.is_chance() {
            return self.chance_mean_of(s, |c| self.reply_value(c));
        }
        if s.phase() == bp::DISCARD || s.phase() == bp::ROBBER {
            let side: f32 = if s.mover() == v { 1.0 } else { -1.0 };
            let acts = self.legal(s);
            let mut best = -2.0f32;
            let mut pick: Option<Node> = None;
            for &a in acts.as_slice() {
                let c = s.child(a);
                let y = if c.is_chance() { self.chance_mean_of(&c, |cc| self.value(cc)) } else { self.value(&c) };
                if side * y > best {
                    best = side * y;
                    pick = Some(c);
                }
            }
            return match pick {
                Some(c) => self.reply_value(&c),
                None => self.leaf(s),
            };
        }
        if s.phase() != bp::MAIN || s.mover() == v {
            return self.leaf(s);
        }
        if self.public_winning_purchase(s) {
            return 0.0;
        }
        let mut frontier: Vec<Node> = vec![*s];
        let mut best = 1.0f32;
        for _ in 0..4 {
            if frontier.is_empty() {
                break;
            }
            let mut cands: Vec<(Node, f32)> = Vec::new();
            for st in &frontier {
                for &a in self.legal(st).as_slice() {
                    if !(a < 180 || a == END_TURN || a >= 229) {
                        continue;
                    }
                    let next = st.child(a);
                    if next.done() {
                        best = best.min(self.leaf(&next));
                        if best == 0.0 {
                            return 0.0;
                        }
                    } else if next.mover() == v {
                        best = best.min(self.leaf(&next));
                    } else {
                        let rank = self.value(&next.child(END_TURN));
                        best = best.min(rank);
                        cands.push((next, rank));
                    }
                }
            }
            cands.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
            let mut kept: Vec<Node> = Vec::new();
            for (n, _) in cands {
                if kept.len() >= 2 {
                    break;
                }
                if !kept.iter().any(|k| same_plan(&k.pos, &n.pos)) {
                    kept.push(n);
                }
            }
            frontier = kept;
        }
        best.clamp(0.0, 1.0)
    }

    /// funded(s, cost) of 7.3.
    fn funded(&self, s: &Node, cost: [u8; 5]) -> Option<Node> {
        let u = s.mover() as usize;
        let st = &s.pos.st;
        for r in 0..5 {
            if (st.bank[r] as i32) < (cost[r] as i32 - st.resources[u][r] as i32).max(0) {
                return None;
            }
        }
        let mut s = *s;
        for t in 0..5 {
            while s.pos.st.resources[u][t] < cost[t] {
                let hand = s.pos.st.resources[u];
                let ratio = s.pos.st.cache.trade_rates[u];
                let g = (0..5).find(|&r| hand[r] as i32 - cost[r] as i32 >= ratio[r] as i32)?;
                if s.done() {
                    return None;
                }
                s = s.child(trade_id(g as u8, t as u8));
            }
        }
        if s.done() { None } else { Some(s) }
    }

    /// Public winning purchase of the rival at its main phase (7.3).
    pub fn public_winning_purchase(&self, s: &Node) -> bool {
        let u = s.mover();
        let vp = s.pos.st.cache.visible_points[u as usize] as u32;
        let target = s.pos.st.config.victory_points as u32;
        if vp + 3 < target {
            return false;
        }
        const COSTS: [[u8; 5]; 3] = [[1, 1, 0, 0, 0], [1, 1, 1, 1, 0], [0, 0, 0, 2, 3]];
        for k in 0..3 {
            if let Some(b) = self.funded(s, COSTS[k]) {
                for &a in self.legal(&b).as_slice() {
                    if a >= 180 {
                        continue;
                    }
                    let kind = if a < 54 { 1 } else if a < 126 { 0 } else { 2 };
                    if kind == k && b.child(a).pos.winner() == Some(u) {
                        return true;
                    }
                }
            }
        }
        let pc = Pieces::of(&s.pos);
        if vp + 2 >= target && pc.city[u as usize].count_ones() <= 2 && pc.settle[u as usize].count_ones() >= 2 {
            if let Some(mut b) = self.funded(s, [0, 0, 0, 4, 6]) {
                for _ in 0..2 {
                    let Some(&a) = self.legal(&b).as_slice().iter().find(|&&a| (126..180).contains(&a)) else {
                        break;
                    };
                    b = b.child(a);
                    if b.pos.winner() == Some(u) {
                        return true;
                    }
                }
            }
        }
        false
    }

    // ---------------- Plain one-step decisions (7.4) ----------------

    pub fn after(&self, s: &Node, depth: u32) -> f32 {
        if s.pos.is_terminal() || !s.is_chance() {
            return self.node1(s, depth);
        }
        self.chance_mean_of(s, |c| self.node1(c, depth))
    }

    pub fn node1(&self, s: &Node, depth: u32) -> f32 {
        if s.pos.is_terminal() || depth == 0 || s.is_chance() || s.mover() != self.v() {
            return self.leaf(s);
        }
        let mut best = 0.0f32;
        for &a in self.legal(s).as_slice() {
            best = best.max(self.after(&s.child(a), depth - 1));
        }
        best
    }

    /// pre-step: the best action by after(child, 1), and its value.
    pub fn pre_step(&self, s: &Node) -> (Option<u8>, f32) {
        let mut best = -2.0f32;
        let mut pick = None;
        for &a in self.legal(s).as_slice() {
            let y = self.after(&s.child(a), 1);
            if y > best {
                best = y;
                pick = Some(a);
            }
        }
        (pick, best)
    }

    /// finish-before-roll F.
    pub fn finish_before_roll(&self, s: &Node) -> f32 {
        if s.done() || s.mover() != self.v() || !s.pos.before_roll {
            return self.leaf(s);
        }
        match s.phase() {
            bp::DICE => self.after(s, 1),
            bp::STEAL => self.chance_mean_of(s, |c| self.finish_before_roll(c)),
            bp::ROBBER | bp::ROAD_BUILDING => match self.pre_step(s).0 {
                Some(a) => self.finish_before_roll(&s.child(a)),
                None => self.leaf(s),
            },
            _ => self.leaf(s),
        }
    }
}
