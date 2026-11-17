//! Viewpoints and the 351 network inputs (BOT-SPEC 3).

use crate::board::BoardInfo;
use crate::geom::{bits64, bits128, geometry, ways};
use crate::position::{Pos, bp};

pub const N_INPUTS: usize = 351;
pub type Inputs = [u8; N_INPUTS];

/// The hidden-card facts of a viewpoint: rival cards H, rival new cards N,
/// true deck count D.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Hidden {
    pub h: u8,
    pub n: u8,
    pub d: u8,
}

/// Make the viewpoint of `pos` for viewer `v` (BOT-SPEC 3.1): the rival's
/// development cards move into the deck, which becomes the unseen pool.
pub fn viewpoint(pos: &Pos, v: u8) -> (Pos, Hidden) {
    let r = (1 - v) as usize;
    let st = &pos.st;
    let h: u8 = (0..5).map(|k| st.playable_development[r][k] + st.new_development[r][k]).sum();
    let n: u8 = st.new_development[r].iter().sum();
    let d: u8 = st.development_deck.iter().sum();
    let mut out = *pos;
    if h > 0 {
        for k in 0..5 {
            out.st.development_deck[k] += st.playable_development[r][k] + st.new_development[r][k];
        }
        out.st.playable_development[r] = [0; 5];
        out.st.new_development[r] = [0; 5];
        out.st.recompute_caches();
    }
    (out, Hidden { h, n, d })
}

const COSTS: [[u8; 5]; 4] = [[1, 1, 0, 0, 0], [1, 1, 1, 1, 0], [0, 0, 0, 2, 3], [0, 0, 1, 1, 1]];
const WR: [f64; 5] = [1.0, 1.0, 1.0, 1.5, 2.0];

#[inline]
fn rule_a(x: f64) -> u8 {
    (100.0 * x).round().clamp(0.0, 255.0) as u8
}
#[inline]
fn rule_b(x: f64) -> u8 {
    (100.0 * x + 0.5).trunc().min(255.0) as u8
}

/// Divisors with a table of `rule_a(n / d)`.
const TAB_DIVS: [u32; 13] = [2, 3, 4, 5, 6, 10, 15, 19, 20, 25, 36, 200, 1];

/// `rule_a(n as f64 / d as f64)` for n in 0..=255 and the divisors of
/// `TAB_DIVS`, and for d = 1..=25 (the unseen pool), each computed by that
/// same expression.
struct RuleTables {
    by_div: [[u8; 256]; 13],
    small: [[u8; 256]; 26],
}

fn rule_tables() -> &'static RuleTables {
    static T: std::sync::OnceLock<RuleTables> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = RuleTables { by_div: [[0; 256]; 13], small: [[0; 256]; 26] };
        for (k, &d) in TAB_DIVS.iter().enumerate() {
            for n in 0..256 {
                t.by_div[k][n] = rule_a(n as f64 / d as f64);
            }
        }
        for d in 1..26 {
            for n in 0..256 {
                t.small[d][n] = rule_a(n as f64 / d as f64);
            }
        }
        t
    })
}

/// `rule_a(n as f64 / D as f64)` for the divisor at index `K` of `TAB_DIVS`.
#[inline]
fn ra<const K: usize>(t: &RuleTables, n: i64) -> u8 {
    if (0..256).contains(&n) { t.by_div[K][n as usize] } else { rule_a(n as f64 / TAB_DIVS[K] as f64) }
}

/// `rule_a(b as u8 as f64)`.
#[inline]
fn ra_bool(b: bool) -> u8 {
    if b { 100 } else { 0 }
}

/// Funding time (BOT-SPEC 3.2).
pub fn fund(hand: &[u8; 5], income: &[f64; 5], ratio: &[u8; 5], cost: &[u8; 5]) -> u32 {
    for t in 0..=10u32 {
        let mut missing = 0.0f64;
        let mut spare = 0.0f64;
        for r in 0..5 {
            let have = hand[r] as f64 + (2 * t) as f64 * income[r];
            let c = cost[r] as f64;
            if have < c {
                missing += c - have;
            } else if t == 0 {
                spare += ((hand[r] - cost[r]) / ratio[r]) as f64;
            } else {
                spare += (have - c) / ratio[r] as f64;
            }
        }
        if spare >= missing {
            return t;
        }
    }
    99
}

/// Per-player masks and counts in bot numbering.
#[derive(Clone, Copy, Default)]
pub struct Pieces {
    pub settle: [u64; 2],
    pub city: [u64; 2],
    pub roads: [u128; 2],
}

impl Pieces {
    pub fn of(pos: &Pos) -> Pieces {
        let g = geometry();
        let mut pc = Pieces::default();
        for (en, &x) in pos.st.node_occupancy.iter().enumerate() {
            if x != 0 {
                let p = ((x - 1) % 2) as usize;
                let b = 1u64 << g.node_of_engine[en];
                if x >= 3 {
                    pc.city[p] |= b;
                } else {
                    pc.settle[p] |= b;
                }
            }
        }
        for (ee, &x) in pos.st.edge_occupancy.iter().enumerate() {
            if x != 0 {
                pc.roads[(x - 1) as usize] |= 1u128 << g.edge_of_engine[ee];
            }
        }
        pc
    }
    #[inline]
    pub fn built(&self, p: usize) -> u64 {
        self.settle[p] | self.city[p]
    }
    /// Nodes touched by a road of p.
    pub fn ends(&self, p: usize) -> u64 {
        let g = geometry();
        let mut m = 0u64;
        for e in bits128(self.roads[p]) {
            let [a, b] = g.edge_nodes[e];
            m |= (1u64 << a) | (1u64 << b);
        }
        m
    }
    /// All occupied nodes and their neighbours.
    pub fn blocked(&self) -> u64 {
        let g = geometry();
        let occ = self.built(0) | self.built(1);
        let mut m = occ;
        for n in bits64(occ) {
            m |= g.node_neighbors[n];
        }
        m
    }
    /// weight(p, t).
    #[inline]
    pub fn weight(&self, p: usize, t: usize) -> u32 {
        let tm = geometry().tile_mask[t];
        (self.settle[p] & tm).count_ones() + 2 * (self.city[p] & tm).count_ones()
    }
}

/// Nodes reached from a node of `f` along one edge not in `roads`.
pub fn expand(f: u64, roads: u128) -> u64 {
    let g = geometry();
    let mut x = 0u64;
    for n in bits64(f) {
        for &e in g.edges_of(n) {
            if roads >> e & 1 == 0 {
                let [a, b] = g.edge_nodes[e as usize];
                x |= 1u64 << (if a as usize == n { b } else { a });
            }
        }
    }
    x
}

fn max_pips(bi: &BoardInfo, m: u64) -> u32 {
    bits64(m).map(|n| bi.pips[n]).max().unwrap_or(0)
}


/// The inputs of one player that depend only on the pieces, the robber and the board.
/// Every byte and sum is computed exactly as in the full computation, so reuse gives
/// the same inputs.
#[derive(Clone, Copy)]
struct PlayerFeat {
    /// Player block (3.4) with the piece-only slots filled.
    pblk: [u8; 66],
    /// Build block (3.5) with the profile slots 0..20 filled.
    bblk: [u8; 24],
    /// Board block (3.6) with the piece-only slots filled.
    dblk: [u8; 48],
    ginc: [f64; 5],
    binc: [f64; 5],
    /// (weight, number, resource) of the tiles with weight > 0, a resource and no robber, in tile order.
    tiles: [[u8; 3]; 19],
    ntiles: u8,
    exp_max: u32,
    nset: u32,
    /// First pick of 3.6: (d1, d2, raced).
    pick: Option<(u8, u8, bool)>,
    near: [u8; 6],
    /// Recent hand-dependent bytes of this player under these pieces.
    hands: [HandBytes; HAND_WAYS],
    next_hand: u8,
}

const HAND_WAYS: usize = 4;

/// The inputs of a player that depend only on its hand, its trade rates, the
/// discard limit and its piece features.
#[derive(Clone, Copy)]
struct HandBytes {
    /// hand, trade rates, discard limit; `valid` false for an empty way.
    key: [u8; 11],
    valid: bool,
    /// Player block 10..22, 46..50, 57..62.
    p10: [u8; 12],
    p46: [u8; 4],
    p57: [u8; 5],
    /// Build block 20..24.
    fund: [u8; 4],
    /// Board block 18 and 42..48.
    d18: u8,
    d42: [u8; 6],
}

const NO_HAND: HandBytes =
    HandBytes { key: [0; 11], valid: false, p10: [0; 12], p46: [0; 4], p57: [0; 5], fund: [0; 4], d18: 0, d42: [0; 6] };

#[derive(Clone, Copy)]
struct PieceFeat {
    pl: [PlayerFeat; 2],
    /// Inputs 223..233.
    gblk: [u8; 10],
    /// Input 42.
    open_byte: u8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct PieceKey {
    settle: [u64; 2],
    city: [u64; 2],
    roads: [u128; 2],
    robber: u8,
}

impl PieceKey {
    #[inline]
    fn slot(&self, bits: u32) -> usize {
        let mut h = self.settle[0].wrapping_mul(0x9e37_79b9_7f4a_7c15);
        for w in [
            self.settle[1],
            self.city[0],
            self.city[1],
            self.roads[0] as u64,
            (self.roads[0] >> 64) as u64,
            self.roads[1] as u64,
            (self.roads[1] >> 64) as u64,
            self.robber as u64,
        ] {
            h = (h.rotate_left(23) ^ w).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        }
        (h >> (64 - bits)) as usize
    }
}

/// Board facts that the piece features read; the cache is flushed when they change.
#[derive(Clone, Copy, PartialEq, Eq)]
struct BoardSig {
    res: [u8; 19],
    num: [u8; 19],
    node_port: [u8; 54],
}

const CACHE_BITS: u32 = 10;

struct PieceCache {
    board: Option<BoardSig>,
    slots: Vec<Option<(PieceKey, PieceFeat)>>,
}

thread_local! {
    static PIECE_CACHE: std::cell::RefCell<PieceCache> =
        const { std::cell::RefCell::new(PieceCache { board: None, slots: Vec::new() }) };
}

fn piece_feats(pc: &Pieces, robber: usize, bi: &BoardInfo) -> PieceFeat {
    let g = geometry();
    let blocked = pc.blocked();
    let open = !blocked & ((1u64 << 54) - 1);
    let ends = [pc.ends(0), pc.ends(1)];
    let all_roads = pc.roads[0] | pc.roads[1];
    let empty = PlayerFeat {
        pblk: [0; 66],
        bblk: [0; 24],
        dblk: [0; 48],
        ginc: [0.0; 5],
        binc: [0.0; 5],
        tiles: [[0; 3]; 19],
        ntiles: 0,
        exp_max: 0,
        nset: 0,
        pick: None,
        near: [9; 6],
        hands: [NO_HAND; HAND_WAYS],
        next_hand: 0,
    };
    let mut pf = PieceFeat { pl: [empty; 2], gblk: [0; 10], open_byte: rule_a(open.count_ones() as f64 / 10.0) };

    // ---- 3.4 piece-only player inputs ----
    for p in 0..2 {
        let q = 1 - p;
        let f = &mut pf.pl[p];
        let o = &mut f.pblk;
        let nset = pc.settle[p].count_ones();
        f.nset = nset;
        o[3] = rule_a(nset as f64 / 5.0);
        o[4] = rule_a(pc.city[p].count_ones() as f64 / 4.0);
        o[5] = rule_a(pc.roads[p].count_ones() as f64 / 15.0);
        let mut inc = [0f64; 5];
        let mut rob = 0f64;
        for tt in 0..19 {
            let w = pc.weight(p, tt);
            let res = bi.res[tt];
            if w == 0 || res == 5 {
                continue;
            }
            let num = bi.num[tt];
            let c = ways(num) as f64 / 36.0;
            let wf = w as f64;
            if tt == robber {
                rob += wf * c;
            } else {
                inc[res as usize] += wf * c;
                f.tiles[f.ntiles as usize] = [w as u8, num, res];
                f.ntiles += 1;
            }
        }
        for r in 0..5 {
            o[22 + r] = rule_a(3.0 * inc[r]);
        }
        o[27] = rule_a(3.0 * rob);
        o[33] = rule_a(inc.iter().filter(|&&x| x > 0.0).count() as f64 / 5.0);
        let mut all = 0.0f64;
        for r in 0..5 {
            all += inc[r];
        }
        o[34] = rule_a(1.5 * all);
        let s_set = ends[p] & !blocked;
        o[50] = rule_a(s_set.count_ones().min(4) as f64 / 4.0);
        o[51] = rule_a(max_pips(bi, s_set) as f64 / 15.0);
        {
            let src = pc.built(p) | (ends[p] & !pc.built(q));
            let mut em = 0u128;
            for n in bits64(src) {
                for &e in g.edges_of(n) {
                    em |= 1u128 << e;
                }
            }
            em &= !all_roads;
            o[52] = rule_a(em.count_ones().min(10) as f64 / 10.0);
        }
        {
            let walls = pc.built(q);
            let mut f2 = (pc.built(p) | ends[p]) & !walls;
            let mut seen = f2;
            let mut mx = 0u32;
            for step in 0..2 {
                let x = expand(f2, all_roads) & !seen;
                seen |= x;
                f2 = x & !walls;
                let xb = x & !blocked;
                let m = max_pips(bi, xb);
                mx = mx.max(m);
                o[53 + 2 * step] = rule_a(xb.count_ones().min(6) as f64 / 6.0);
                o[54 + 2 * step] = rule_a(m as f64 / 15.0);
            }
            f.exp_max = mx;
        }
        o[65] = rule_a(max_pips(bi, pc.settle[p]) as f64 / 15.0);
    }

    // ---- 3.5 build profiles ----
    let mut prod = [[0f64; 5]; 54];
    for tt in 0..19 {
        let res = bi.res[tt];
        if res == 5 || tt == robber {
            continue;
        }
        let c = ways(bi.num[tt]) as f64 / 36.0;
        for &n in &g.tile_nodes[tt] {
            prod[n as usize][res as usize] += c;
        }
    }
    for p in 0..2 {
        let q = 1 - p;
        let mut ginc = [0f64; 5];
        for n in bits64(pc.built(p)) {
            let cnt = if pc.city[p] >> n & 1 == 1 { 2.0 } else { 1.0 };
            for r in 0..5 {
                ginc[r] += cnt * prod[n][r];
            }
        }
        let mut basev = [0f64; 5];
        for r in 0..5 {
            basev[r] = (4.0 * ginc[r]).ln_1p();
        }
        let profile = |c: u64| -> [f64; 5] {
            if c == 0 {
                return [0.0; 5];
            }
            let mut best_s = -1.0f64;
            let mut best = usize::MAX;
            for n in bits64(c) {
                let mut s = 0.0f64;
                for r in 0..5 {
                    if prod[n][r] != 0.0 {
                        s += WR[r] * ((4.0 * (ginc[r] + prod[n][r])).ln_1p() - basev[r]);
                    }
                }
                if s > best_s {
                    best_s = s;
                    best = n;
                }
            }
            let mut o = [0f64; 5];
            if best != usize::MAX {
                for r in 0..5 {
                    o[r] = 3.0 * prod[best][r];
                }
            }
            o
        };
        let f = &mut pf.pl[p];
        f.ginc = ginc;
        let ncity = pc.city[p].count_ones();
        let nset = pc.settle[p].count_ones();
        let nroad = pc.roads[p].count_ones();
        let pr = profile(if ncity < 4 { pc.settle[p] } else { 0 });
        for r in 0..5 {
            f.bblk[r] = rule_a(pr[r]);
        }
        let walls = pc.built(q);
        let mut fr = (pc.built(p) | ends[p]) & !walls;
        let mut seen = fr;
        for step in 0..3u32 {
            let c = if nset < 5 && nroad + step <= 15 { fr & !blocked } else { 0 };
            let pr = profile(c);
            for r in 0..5 {
                f.bblk[5 + 5 * step as usize + r] = rule_a(pr[r]);
            }
            let x = expand(fr, all_roads) & !seen;
            seen |= x;
            fr = x & !walls;
        }
    }

    // ---- 3.6 board inputs (rule B) ----
    let mut supply = [0f64; 5];
    for tt in 0..19 {
        let res = bi.res[tt];
        if res != 5 {
            supply[res as usize] += ways(bi.num[tt]) as f64 / 36.0;
        }
    }
    for r in 0..5 {
        pf.gblk[r] = rule_b(2.0 * supply[r]);
    }
    let mut top4 = [0u32; 4];
    let mut rich = 0u32;
    for n in bits64(open) {
        let mut carry = bi.pips[n];
        for slot in &mut top4 {
            if carry > *slot {
                std::mem::swap(&mut carry, slot);
            }
        }
        if bi.pips[n] >= 10 {
            rich += 1;
        }
    }
    for i in 0..4 {
        pf.gblk[5 + i] = rule_b(top4[i] as f64 / 15.0);
    }
    pf.gblk[9] = rule_b(rich.min(6) as f64 / 6.0);

    let dist_of = |p: usize| -> [u8; 54] {
        let q = 1 - p;
        let mut dist = [9u8; 54];
        let walls = pc.built(q);
        let mut f = (pc.built(p) | ends[p]) & !walls;
        let mut seen = f;
        for step in 0..=5u8 {
            if f == 0 {
                break;
            }
            for n in bits64(f) {
                dist[n] = step;
            }
            let x = expand(f, all_roads) & !seen;
            seen |= x;
            f = x & !walls;
        }
        dist
    };
    let dists = [dist_of(0), dist_of(1)];
    for p in 0..2 {
        let mut binc = [0f64; 5];
        for tt in 0..19 {
            let res = bi.res[tt];
            if res == 5 || tt == robber {
                continue;
            }
            let w = pc.weight(p, tt);
            binc[res as usize] += (w as f64 * ways(bi.num[tt]) as f64) / 36.0;
        }
        pf.pl[p].binc = binc;
    }
    for p in 0..2 {
        let q = 1 - p;
        let mine = &dists[p];
        let theirs = &dists[q];
        let mut far = [0u32; 2];
        let mut top = [0u32; 2];
        let mut sec_n = 0u32;
        let mut sec_sum = 0u32;
        let mut sec_max = 0u32;
        let mut con_n = 0u32;
        let mut con_max = 0u32;
        let mut cands = [(0usize, 0i32); 54];
        let mut ncand = 0usize;
        let mut near = [9u8; 6];
        for n in bits64(open) {
            let d = mine[n];
            if d > 5 {
                continue;
            }
            let pips = bi.pips[n];
            if d == 3 || d == 4 {
                far[(d - 3) as usize] += 1;
                top[(d - 3) as usize] = top[(d - 3) as usize].max(pips);
            }
            if d < theirs[n] {
                sec_n += 1;
                sec_sum += pips;
                sec_max = sec_max.max(pips);
            } else if d == theirs[n] {
                con_n += 1;
                con_max = con_max.max(pips);
            }
            if d <= theirs[n] && d <= 4 {
                cands[ncand] = (n, pips as i32 - 2 * d as i32);
                ncand += 1;
            }
            let pk = bi.node_port[n];
            if pk != 255 {
                near[pk as usize] = near[pk as usize].min(d);
            }
        }
        let cands = &cands[..ncand];
        let nset_p = pc.settle[p].count_ones();
        let room = 5 - nset_p;
        let spare = 15 - pc.roads[p].count_ones();
        let mut chosen = 0u64;
        let mut used = 0u32;
        let mut built = 0u32;
        let mut gain = [0f64; 5];
        let mut taken = [false; 54];
        let mut picks = [0usize; 2];
        let mut npicks = 0usize;
        const FADE: [f64; 5] = [1.0, 0.8, 0.64, 0.51, 0.41];
        while built < room {
            let mut best: Option<usize> = None;
            for (i, &(n, val)) in cands.iter().enumerate() {
                if taken[i] || g.node_neighbors[n] & chosen != 0 {
                    continue;
                }
                if used + mine[n] as u32 > spare {
                    continue;
                }
                if best.is_none_or(|b| val > cands[b].1) {
                    best = Some(i);
                }
            }
            let Some(i) = best else { break };
            let n = cands[i].0;
            let d = mine[n] as u32;
            if npicks < 2 {
                picks[npicks] = n;
                npicks += 1;
            }
            taken[i] = true;
            chosen |= 1u64 << n;
            used += d;
            built += 1;
            for &tt in g.tiles_of(n) {
                let tt = tt as usize;
                let res = bi.res[tt];
                if res == 5 || tt == robber {
                    continue;
                }
                gain[res as usize] += (FADE[d as usize] * ways(bi.num[tt]) as f64) / 36.0;
            }
        }
        let f = &mut pf.pl[p];
        f.near = near;
        let o = &mut f.dblk;
        o[0] = rule_b(far[0].min(6) as f64 / 6.0);
        o[1] = rule_b(top[0] as f64 / 15.0);
        o[2] = rule_b(far[1].min(6) as f64 / 6.0);
        o[3] = rule_b(top[1] as f64 / 15.0);
        o[4] = rule_b(sec_n.min(8) as f64 / 8.0);
        o[5] = rule_b(sec_sum.min(80) as f64 / 40.0);
        o[6] = rule_b(sec_max as f64 / 15.0);
        o[7] = rule_b(con_n.min(4) as f64 / 4.0);
        o[8] = rule_b(con_max as f64 / 15.0);
        o[9] = rule_b(built as f64 / 4.0);
        o[10] = rule_b(used as f64 / 8.0);
        for r in 0..5 {
            o[11 + r] = rule_b(3.0 * gain[r]);
        }
        if npicks >= 1 {
            let n1 = picks[0];
            let d1 = mine[n1];
            let d2 = theirs[n1];
            let raced = d2 <= 5 && pc.settle[q].count_ones() < 5;
            o[16] = rule_b(bi.pips[n1] as f64 / 15.0);
            o[17] = rule_b(d1 as f64 / 4.0);
            o[19] = rule_b(if raced { (6.0 - d2 as f64) / 6.0 } else { 0.0 });
            f.pick = Some((d1, d2, raced));
        } else {
            o[16] = rule_b(0.0);
            o[17] = rule_b(0.0);
            o[18] = rule_b(1.0);
            o[19] = rule_b(0.0);
            o[20] = rule_b(1.0);
            f.pick = None;
        }
        if npicks >= 2 {
            let n2 = picks[1];
            o[21] = rule_b(bi.pips[n2] as f64 / 15.0);
            o[22] = rule_b(mine[n2] as f64 / 4.0);
        } else {
            o[21] = 0;
            o[22] = 0;
        }
        let mut all = 0f64;
        let mut shared = 0f64;
        let mut exposed = 0f64;
        let mut topy = 0f64;
        let mut second = 0f64;
        let mut bynum = [0u32; 13];
        let bp_ = pc.built(p);
        let bq = pc.built(q);
        for tt in 0..19 {
            let tm = g.tile_mask[tt];
            let res = bi.res[tt];
            if bp_ & tm == 0 || res == 5 {
                continue;
            }
            let w = pc.weight(p, tt);
            let num = bi.num[tt];
            let y = (w as f64 * ways(num) as f64) / 36.0;
            all += y;
            if bq & tm != 0 {
                shared += y;
            } else {
                exposed = exposed.max(y);
            }
            if y > topy {
                second = topy;
                topy = y;
            } else {
                second = second.max(y);
            }
            if tt != robber {
                bynum[num as usize] += w;
            }
        }
        o[23] = rule_b(3.0 * topy);
        o[24] = rule_b(3.0 * second);
        o[25] = rule_b(if all > 0.0 { shared / all } else { 0.0 });
        o[26] = rule_b(3.0 * exposed);
        for (j, nn) in [2usize, 3, 4, 5, 6, 8, 9, 10, 11, 12].into_iter().enumerate() {
            o[27 + j] = rule_b(bynum[nn].min(8) as f64 / 4.0);
        }
    }
    pf
}

/// The piece features of `pc` under `robber`, from the thread's cache when present.
#[inline]
fn with_piece_feats<R>(pc: &Pieces, robber: usize, bi: &BoardInfo, f: impl FnOnce(&mut PieceFeat) -> R) -> R {
    let key = PieceKey { settle: pc.settle, city: pc.city, roads: pc.roads, robber: robber as u8 };
    let sig = BoardSig { res: bi.res, num: bi.num, node_port: bi.node_port };
    PIECE_CACHE.with(|cell| {
        let mut c = cell.borrow_mut();
        if c.board != Some(sig) {
            c.board = Some(sig);
            c.slots.clear();
            c.slots.resize(1 << CACHE_BITS, None);
        }
        let i = key.slot(CACHE_BITS);
        match &mut c.slots[i] {
            Some((k, pf)) if *k == key => f(pf),
            slot => {
                let (_, pf) = slot.insert((key, piece_feats(pc, robber, bi)));
                f(pf)
            }
        }
    })
}

/// Compute the 351 inputs of a viewpoint (`pos` already hides the rival's cards).
pub fn compute_inputs(pos: &Pos, v: u8, hid: Hidden, bi: &BoardInfo, out: &mut Inputs) {
    let g = geometry();
    let st = &pos.st;
    let pc = Pieces::of(pos);
    let robber = g.tile_of_engine[st.robber_hex as usize] as usize;
    with_piece_feats(&pc, robber, bi, |pf| fill_inputs(pos, v, hid, pf, out));
}

/// The hand-dependent bytes of player p (see `HandBytes`).
fn hand_bytes(f: &PlayerFeat, hand: &[u8; 5], ratio: &[u8; 5], discard_limit: u8) -> HandBytes {
    let mut h = NO_HAND;
    let o = &mut h.p10;
    for r in 0..5 {
        o[r] = rule_a(hand[r].min(12) as f64 / 6.0);
    }
    let total: u32 = hand.iter().map(|&x| x as u32).sum();
    o[5] = rule_a(total as f64 / 20.0);
    o[6] = rule_a((total > discard_limit as u32) as u8 as f64);
    for r in 0..5 {
        o[7 + r] = rule_a((4.0 - ratio[r] as f64) / 2.0);
    }
    for ci in 0..4 {
        let ok = (0..5).all(|r| hand[r] >= COSTS[ci][r]);
        h.p46[ci] = rule_a(ok as u8 as f64);
    }
    for ci in 0..4 {
        let miss: u32 = (0..5).map(|r| COSTS[ci][r].saturating_sub(hand[r]) as u32).sum();
        h.p57[ci] = rule_a(miss as f64 / 3.0);
    }
    let trades: u32 = (0..5).map(|r| (hand[r] / ratio[r]) as u32).sum();
    h.p57[4] = rule_a(trades.min(4) as f64 / 2.0);
    for ci in 0..4 {
        let t = fund(hand, &f.ginc, ratio, &COSTS[ci]);
        h.fund[ci] = rule_a(t.min(12) as f64 / 12.0);
    }
    if let Some((d1, _, _)) = f.pick {
        let c1 = [1 + d1, 1 + d1, 1, 1, 0];
        h.d18 = rule_b(fund(hand, &f.binc, ratio, &c1).min(12) as f64 / 12.0);
    } else {
        h.d18 = f.dblk[18];
    }
    let near = &f.near;
    h.d42[0] = rule_b(if ratio.iter().all(|&x| x <= 3) {
        1.0
    } else if near[0] <= 4 {
        (5.0 - near[0] as f64) / 6.0
    } else {
        0.0
    });
    for r in 0..5 {
        h.d42[1 + r] = rule_b(if ratio[r] == 2 {
            1.0
        } else if near[r + 1] <= 4 {
            (5.0 - near[r + 1] as f64) / 6.0
        } else {
            0.0
        });
    }
    h
}

/// The hand bytes of player p, from the memo of its piece features when present.
#[inline]
fn hand_memo(f: &mut PlayerFeat, hand: &[u8; 5], ratio: &[u8; 5], discard_limit: u8) -> HandBytes {
    let mut key = [0u8; 11];
    key[..5].copy_from_slice(hand);
    key[5..10].copy_from_slice(ratio);
    key[10] = discard_limit;
    if let Some(h) = f.hands.iter().find(|h| h.valid && h.key == key) {
        return *h;
    }
    let mut h = hand_bytes(f, hand, ratio, discard_limit);
    h.key = key;
    h.valid = true;
    f.hands[f.next_hand as usize] = h;
    f.next_hand = ((f.next_hand as usize + 1) % HAND_WAYS) as u8;
    h
}

/// Next-roll odds and their 11 input bytes, memoized on the last dice facts of this thread.
fn odds_of(pos: &Pos) -> ([f64; 11], [u8; 11]) {
    let st = &pos.st;
    let next_roller = if pos.before_roll || pos.phase == bp::DICE {
        st.turn_owner
    } else {
        1 - st.turn_owner
    };
    let mut d = st.dice;
    d.counts = pos.dice_counts;
    let key = (d, next_roller, st.config.dice);
    ODDS.with(|cell| {
        let mut c = cell.borrow_mut();
        if let Some((k, odds, bytes)) = &*c {
            if *k == key {
                return (*odds, *bytes);
            }
        }
        let mut odds = [0f64; 11];
        let w: [u128; 11] = if st.config.dice == 0 {
            d.reference_probabilities(next_roller).weights
        } else {
            catan_engine::dice::INITIAL_COUNTS.map(|x| x as u128)
        };
        let sum: u128 = w.iter().sum();
        for i in 0..11 {
            odds[i] = w[i] as f64 / sum as f64;
        }
        let mut bytes = [0u8; 11];
        for i in 0..11 {
            bytes[i] = rule_a(odds[i] * 6.0);
        }
        *c = Some((key, odds, bytes));
        (odds, bytes)
    })
}

type OddsMemo = Option<((catan_engine::dice::DiceState, u8, u8), [f64; 11], [u8; 11])>;

thread_local! {
    static ODDS: std::cell::RefCell<OddsMemo> = const { std::cell::RefCell::new(None) };
}

fn fill_inputs(pos: &Pos, v: u8, hid: Hidden, pf: &mut PieceFeat, out: &mut Inputs) {
    let st = &pos.st;
    let me = v as usize;
    let rv = 1 - me;
    let order = [me, rv];
    let pool = st.development_deck;
    let unseen: u32 = pool.iter().map(|&x| x as u32).sum();
    let dice_cards: u32 = pos.dice_counts.iter().map(|&x| x as u32).sum();
    let hh = hid.h as f64;

    // dev, new per player (rival zero).
    let mut dev = [[0u8; 5]; 2];
    let mut newc = [[0u8; 5]; 2];
    for p in 0..2 {
        for k in 0..5 {
            dev[p][k] = st.playable_development[p][k] + st.new_development[p][k];
            newc[p][k] = st.new_development[p][k];
        }
    }
    let played = st.spent_development;
    let ratio = st.cache.trade_rates;
    let hand = st.resources;
    let played_card = |p: usize| st.turn_owner as usize == p && st.development_played;
    let (odds, odds_bytes) = odds_of(pos);
    let hb = [
        hand_memo(&mut pf.pl[0], &hand[0], &ratio[0], st.config.discard_limit),
        hand_memo(&mut pf.pl[1], &hand[1], &ratio[1], st.config.discard_limit),
    ];
    let pf = &*pf;
    // Remaining-dice share of a number, as in the player and board blocks.
    let share = |num: u8| -> f64 {
        if dice_cards >= 13 {
            pos.dice_counts[(num - 2) as usize] as f64 / dice_cards as f64
        } else {
            ways(num) as f64 / 36.0
        }
    };

    // ---- 3.3 global ----
    // Index of each divisor in TAB_DIVS.
    const D2: usize = 0;
    const D3: usize = 1;
    const D4: usize = 2;
    const D5: usize = 3;
    const D6: usize = 4;
    const D10: usize = 5;
    const D15: usize = 6;
    const D19: usize = 7;
    const D25: usize = 9;
    const D36: usize = 10;
    const D200: usize = 11;
    let tab = rule_tables();
    for i in 0..=10 {
        out[i] = ra_bool(pos.phase as usize == i);
    }
    out[11] = ra_bool(pos.mover as usize == me);
    out[12] = ra_bool(st.turn_owner as usize == me);
    out[13] = ra_bool(pos.before_roll);
    out[14] = ra::<D4>(tab, pos.setup_placed as i64);
    out[15..26].copy_from_slice(&odds_bytes);
    out[26] = ra::<D36>(tab, dice_cards as i64);
    for r in 0..5 {
        out[27 + r] = ra::<D19>(tab, st.bank[r] as i64);
    }
    out[32] = ra::<D25>(tab, hid.d as i64);
    let t = if st.turn_number == 0 { 0 } else { st.turn_number - 1 };
    out[33] = ra::<D200>(tab, t.min(200) as i64);
    out[34] = ra::<D2>(tab, pos.free_roads_left as i64);
    out[35] = ra::<D10>(tab, st.discard_remaining[me] as i64);
    out[36] = ra::<D10>(tab, st.discard_remaining[rv] as i64);
    for k in 0..5 {
        out[37 + k] = if unseen == 0 {
            rule_a(0.0)
        } else if unseen < 26 {
            tab.small[unseen as usize][pool[k] as usize]
        } else {
            rule_a(pool[k] as f64 / unseen as f64)
        };
    }
    out[42] = pf.open_byte;

    // ---- 3.4 player inputs ----
    for (blk, &p) in order.iter().enumerate() {
        let q = 1 - p;
        let f = &pf.pl[p];
        let mine = p == me;
        let base = 43 + 66 * blk;
        let o = &mut out[base..base + 66];
        o.copy_from_slice(&f.pblk);
        // Visible points and held point cards are whole numbers, so the f64
        // sums and differences of the definition are these integers exactly.
        let vp = st.cache.visible_points[p] as i64;
        let vp_cards = if mine { dev[p][1] as i64 } else { 0 };
        let pts = vp + vp_cards;
        o[0] = ra::<D15>(tab, vp);
        o[1] = ra::<D15>(tab, pts);
        o[2] = ra::<D15>(tab, st.config.victory_points as i64 - pts);
        o[6] = ra::<D15>(tab, st.cache.road_lengths[p] as i64);
        o[7] = ra_bool(st.longest_road_holder as usize == p);
        o[8] = ra_bool(st.largest_army_holder as usize == p);
        o[9] = ra::<D6>(tab, played[p][0].min(6) as i64);
        let h = &hb[p];
        o[10..22].copy_from_slice(&h.p10);
        o[46..50].copy_from_slice(&h.p46);
        o[57..62].copy_from_slice(&h.p57);
        let mut nxt = [0f64; 5];
        let mut rem = 0f64;
        for &[w, num, res] in &f.tiles[..f.ntiles as usize] {
            let wf = w as f64;
            nxt[res as usize] += wf * odds[(num - 2) as usize];
            rem += wf * share(num);
        }
        for r in 0..5 {
            o[28 + r] = rule_a(3.0 * nxt[r]);
        }
        if mine {
            for (j, k) in [0usize, 2, 3, 4].into_iter().enumerate() {
                o[35 + j] = ra::<D3>(tab, ((dev[p][k] - newc[p][k]).min(3)) as i64);
            }
            o[39] = ra::<D3>(tab, dev[p][1] as i64);
            let s: u32 = newc[p].iter().map(|&x| x as u32).sum();
            o[40] = ra::<D3>(tab, s as i64);
            o[41] = 0;
        } else {
            for j in 0..5 {
                o[35 + j] = 0;
            }
            o[40] = ra::<D3>(tab, hid.n as i64);
            o[41] = ra::<D4>(tab, hid.h.min(8) as i64);
        }
        for (j, k) in [2usize, 3, 4].into_iter().enumerate() {
            o[42 + j] = ra::<D2>(tab, played[p][k] as i64);
        }
        o[45] = ra_bool(played_card(p));
        let k = played[p][0] as f64
            + if mine {
                dev[p][0] as f64
            } else if unseen == 0 {
                0.0
            } else {
                (hh * pool[0] as f64) / unseen as f64
            };
        o[62] = rule_a(k.min(6.0) / 3.0);
        let lp = st.cache.road_lengths[p] as i32;
        let lq = st.cache.road_lengths[q] as i32;
        o[63] = ra::<D5>(tab, (lp - lq + 5).clamp(0, 10) as i64);
        o[64] = rule_a(1.5 * rem);
    }

    // ---- 3.5 build inputs ----
    for (blk, &p) in order.iter().enumerate() {
        let f = &pf.pl[p];
        let base = 175 + 24 * blk;
        out[base..base + 20].copy_from_slice(&f.bblk[..20]);
        out[base + 20..base + 24].copy_from_slice(&hb[p].fund);
    }

    // ---- 3.6 board inputs (rule B) ----
    out[223..233].copy_from_slice(&pf.gblk);
    for (blk, &p) in order.iter().enumerate() {
        let q = 1 - p;
        let f = &pf.pl[p];
        let base = 233 + 48 * blk;
        let o = &mut out[base..base + 48];
        o.copy_from_slice(&f.dblk);
        o[18] = hb[p].d18;
        if let Some((_, d2, raced)) = f.pick {
            o[20] = rule_b(if raced {
                let c2 = [1 + d2, 1 + d2, 1, 1, 0];
                fund(&hand[q], &pf.pl[q].binc, &ratio[q], &c2).min(12) as f64 / 12.0
            } else {
                1.0
            });
        }
        let mut rem2 = [0f64; 5];
        for &[w, num, res] in &f.tiles[..f.ntiles as usize] {
            rem2[res as usize] += w as f64 * share(num);
        }
        for r in 0..5 {
            o[37 + r] = rule_b(1.5 * rem2[r]);
        }
        o[42..48].copy_from_slice(&hb[p].d42);
    }

    // ---- 3.7 card inputs ----
    for (blk, &p) in order.iter().enumerate() {
        let q = 1 - p;
        let mine = p == me;
        let base = 329 + 11 * blk;
        let mut held = [0f64; 5];
        for k in 0..5 {
            held[k] = if mine {
                dev[p][k] as f64
            } else if unseen == 0 {
                0.0
            } else {
                (hh * pool[k] as f64) / unseen as f64
            };
        }
        let any = |k: usize| held[k].min(1.0);
        let o = &mut out[base..base + 11];
        let qmax = *hand[q].iter().max().unwrap() as f64;
        o[0] = rule_a((any(4) * qmax) / 6.0);
        o[1] = rule_a((any(4) * hand[q][4] as f64) / 6.0);
        o[2] = rule_a((any(4) * hand[q][3] as f64) / 6.0);
        for (j, ci) in [2usize, 1, 3].into_iter().enumerate() {
            let miss: u32 = (0..5).map(|r| COSTS[ci][r].saturating_sub(hand[p][r]) as u32).sum();
            o[3 + j] = rule_a(if miss == 1 || miss == 2 { any(3) } else { 0.0 });
        }
        o[6] = rule_a(if pf.pl[p].nset < 5 {
            (any(2) * pf.pl[p].exp_max as f64) / 15.0
        } else {
            0.0
        });
        let lp = st.cache.road_lengths[p] as u32;
        let lq = st.cache.road_lengths[q] as u32;
        o[7] = rule_a(if st.longest_road_holder as usize != p && lp + 2 >= 5 && lp + 2 > lq {
            any(2)
        } else {
            0.0
        });
        let diff = (played[p][0] as f64 + held[0] - played[q][0] as f64).clamp(-3.0, 3.0);
        o[8] = rule_a((diff + 3.0) / 6.0);
        let k0 = played[p][0] as u32;
        o[9] = rule_a(
            if st.largest_army_holder as usize != p && k0 + 1 >= 3 && k0 + 1 > played[q][0] as u32 {
                any(0)
            } else {
                0.0
            },
        );
        o[10] = rule_a(held[1] / 3.0);
    }
}
