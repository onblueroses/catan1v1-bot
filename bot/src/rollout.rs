//! Paired rollouts (BOT-SPEC 8.3-8.8).

use crate::belief::Belief;
use crate::board::BoardInfo;
use crate::decide::decide_view;
use crate::inputs::{Hidden, compute_inputs, viewpoint};
use crate::keys::{key181, root_seed, sample_seed};
use crate::net::Nets;
use crate::position::{END_TURN, Pos, bp};
use crate::rng::Rng;
use crate::search::{Node, Settings};
use crate::value::Mode;
use catan_engine::phase as ep;

/// One event of a traced rollout (the fixture trace format).
#[derive(Clone, Debug, PartialEq)]
pub struct TraceEvent {
    pub stage: &'static str,
    pub seed: u64,
    pub detail: i64,
    pub mean: Option<f64>,
    pub realized: Option<f64>,
    pub correction: Option<f64>,
    pub raw: Option<f64>,
    pub value: Option<f64>,
    pub key177: Vec<u8>,
}

impl TraceEvent {
    fn new(stage: &'static str, seed: u64, detail: i64, r: &Pos) -> TraceEvent {
        TraceEvent {
            stage,
            seed,
            detail,
            mean: None,
            realized: None,
            correction: None,
            raw: None,
            value: None,
            key177: key181(r, 0)[..177].to_vec(),
        }
    }
}

pub struct RolloutCtx<'a> {
    pub nets: &'a Nets,
    pub bi: &'a BoardInfo,
    pub set: Settings,
    pub v: u8,
    pub hid: Hidden,
}

const STREAM_DICE: u64 = 0x2dd79a409a716c17;
const STREAM_DRAW: u64 = 0x1cb875d064a0ba11;
const STREAM_THEFT: u64 = 0xe133ab27fc80150d;
const STREAM_PLAYER: [u64; 2] = [0x871893dcc7a5c363, 0x9c18e08134fe6aa5];

impl<'a> RolloutCtx<'a> {
    /// Stop value (8.5) for the root viewer.
    pub fn stop_value(&self, r: &Pos) -> f32 {
        if let Some(w) = r.winner() {
            return if w == self.v { 1.0 } else { 0.0 };
        }
        crate::search::cached(&crate::search::stop_key(r, self.v, self.nets.main.id()), || {
            let (vp, hid) = viewpoint(r, self.v);
            let mut x = [0u8; 351];
            compute_inputs(&vp, self.v, hid, self.bi, &mut x);
            self.nets.main.eval_fixed(&x)
        })
    }

    /// The rollout player's choice for the mover of r (8.6).
    fn player_action(&self, r: &Pos, rng: &mut Rng) -> u8 {
        let u = r.mover;
        let (vp, hid) = viewpoint(r, u);
        let set = Settings::rollout_player();
        decide_view(self.nets, self.bi, &set, &Node { pos: vp, d: hid.d }, u, hid, Mode::Main, rng, None).action
    }

    /// One rollout (8.4). None when cut.
    pub fn rollout(&self, root: &Node, e: &Node, world: &Pos, seed: u64, mut trace: Option<&mut Vec<TraceEvent>>) -> Option<f32> {
        let rv = (1 - self.v) as usize;
        let mut r = e.pos;
        r.st.development_deck = world.st.development_deck;
        r.st.playable_development[rv] = world.st.playable_development[rv];
        r.st.new_development[rv] = [0; 5];
        r.st.recompute_caches();
        if let Some(t) = trace.as_deref_mut() {
            t.push(TraceEvent::new("start", seed, 0, &r));
        }
        let mut dice = Rng::seed(seed ^ STREAM_DICE);
        let mut draw = Rng::seed(seed ^ STREAM_DRAW);
        let mut theft = Rng::seed(seed ^ STREAM_THEFT);
        let mut players = [Rng::seed(seed ^ STREAM_PLAYER[0]), Rng::seed(seed ^ STREAM_PLAYER[1])];
        let root_turn = root.pos.st.turn_number;
        let stop = root_turn + 5;
        let mut counted = r.st.turn_number;
        let mut main = 0u32;
        let mut correction = 0f64;
        let mut steps = 0i64;
        for _ in 0..384 {
            if r.done() || r.st.turn_number >= stop {
                break;
            }
            steps += 1;
            if r.st.turn_number != counted {
                counted = r.st.turn_number;
                main = 0;
            }
            if r.is_chance() {
                let stream = match r.st.phase {
                    ep::DICE => &mut dice,
                    ep::DEVELOPMENT_DRAW => &mut draw,
                    _ => &mut theft,
                };
                let (outs, n) = r.outcomes();
                let weights: Vec<u128> = outs[..n].iter().map(|o| o.1).collect();
                let pick = stream.pick_weighted(&weights);
                let mut m = 0f64;
                let mut t = 0f64;
                let mut y_pick = 0f64;
                let mut next = r;
                for (j, &(o, w)) in outs[..n].iter().enumerate() {
                    let c = r.apply_outcome(o);
                    let y = self.stop_value(&c) as f64;
                    m += w as f64 * y;
                    t += w as f64;
                    if j == pick {
                        y_pick = y;
                        next = c;
                    }
                }
                correction += y_pick - m / t;
                if let Some(tr) = trace.as_deref_mut() {
                    let mut ev = TraceEvent::new("chance", seed, outs[pick].0 as i64, &r);
                    ev.mean = Some(m / t);
                    ev.realized = Some(y_pick);
                    ev.correction = Some(correction);
                    tr.push(ev);
                    tr.push(TraceEvent::new("after_chance", seed, outs[pick].0 as i64, &next));
                }
                r = next;
            } else {
                let a = if r.phase == bp::MAIN && (r.st.turn_number == root_turn || main >= 12) {
                    END_TURN
                } else {
                    let a = self.player_action(&r, &mut players[r.mover as usize]);
                    if r.phase == bp::MAIN {
                        main += 1;
                    }
                    a
                };
                r = r.apply(a);
                if let Some(tr) = trace.as_deref_mut() {
                    tr.push(TraceEvent::new("after_action", seed, a as i64, &r));
                }
            }
        }
        if !r.done() && r.st.turn_number < stop {
            if let Some(tr) = trace.as_deref_mut() {
                tr.push(TraceEvent::new("cut", seed, steps, &r));
            }
            return None;
        }
        let raw = self.stop_value(&r);
        let value = (raw as f64 - correction) as f32;
        if let Some(tr) = trace.as_deref_mut() {
            let mut ev = TraceEvent::new("stop", seed, steps, &r);
            ev.raw = Some(raw as f64);
            ev.correction = Some(correction);
            ev.value = Some(value as f64);
            tr.push(ev);
        }
        Some(value)
    }
}

/// The rollout stage: changes `values` only when every rollout completes.
pub fn rollout_stage(
    ctx: &RolloutCtx,
    root: &Node,
    acts: &[u8],
    values: &mut [f32],
    endpoints: &[Option<Node>],
    mut trace: Option<&mut Vec<TraceEvent>>,
) {
    let _ = acts;
    let v = ctx.v;
    let hid = ctx.hid;
    let mut short: Vec<usize> = (0..values.len()).filter(|&i| values[i] > -1.0).collect();
    short.sort_by(|&a, &b| values[b].partial_cmp(&values[a]).unwrap().then(a.cmp(&b)));
    short.truncate(8);
    if short.is_empty() {
        return;
    }
    let st = &root.pos.st;
    let pool = st.development_deck;
    let pool_sum: u32 = pool.iter().map(|&x| x as u32).sum();
    if root.phase() != bp::MAIN || st.turn_owner != v || root.done() || hid.n != 0 || pool_sum != hid.h as u32 + hid.d as u32 {
        return;
    }
    let rv = (1 - v) as usize;
    let target = st.config.victory_points as i32;
    let cap = target - 1 - st.cache.visible_points[rv] as i32;
    let Some(belief) = Belief::new(pool, hid.h, cap) else { return };
    let key = key181(&root.pos, hid.d);
    let rs = root_seed(&key, v, hid, ctx.bi, root.pos.port_ring);
    let n = ctx.set.rollout_samples.clamp(1, 192);
    let mut worlds: Vec<(u64, Pos)> = Vec::with_capacity(n);
    for i in 0..n {
        let seed = sample_seed(rs, i);
        let Some(c) = belief.sample(seed) else { return };
        let mut w = root.pos;
        w.st.playable_development[rv] = c;
        w.st.new_development[rv] = [0; 5];
        for k in 0..5 {
            w.st.development_deck[k] = pool[k] - c[k];
        }
        w.st.recompute_caches();
        let deck: u32 = w.st.development_deck.iter().map(|&x| x as u32).sum();
        if deck != hid.d as u32 || w.st.cache.visible_points[rv] as i32 + c[1] as i32 >= target {
            return;
        }
        worlds.push((seed, w));
    }
    let k_n = short.len();
    // g[k][i]
    let mut g = vec![vec![0f32; n]; k_n];
    let jobs: Vec<(usize, usize)> = (0..n).flat_map(|i| (0..k_n).map(move |k| (k, i))).collect();
    if let Some(tr) = trace.as_deref_mut() {
        for &(k, i) in &jobs {
            let e = endpoints[short[k]].as_ref().unwrap();
            match ctx.rollout(root, e, &worlds[i].1, worlds[i].0, Some(tr)) {
                Some(x) => g[k][i] = x,
                None => return,
            }
        }
    } else {
        let workers = ctx.set.workers.clamp(1, 8).max(1);
        let results = run_parallel(&jobs, workers, |&(k, i)| {
            let e = endpoints[short[k]].as_ref().unwrap();
            ctx.rollout(root, e, &worlds[i].1, worlds[i].0, None)
        });
        for (j, &(k, i)) in jobs.iter().enumerate() {
            match results[j] {
                Some(x) => g[k][i] = x,
                None => return,
            }
        }
    }
    // Final pick (8.8).
    let nf = n as f64;
    let mut selected = 0usize;
    let mut gain = 0f64;
    for k in 1..k_n {
        let mut s = 0f64;
        let mut q = 0f64;
        for i in 0..n {
            let d = g[k][i] as f64 - g[0][i] as f64;
            s += d;
            q += d * d;
        }
        let err = ((q - s * s / nf).max(0.0) / (nf * (nf - 1.0))).sqrt();
        let adj = s / nf - err;
        if adj > gain {
            gain = adj;
            selected = k;
        }
    }
    let mut sum = 0f64;
    for i in 0..n {
        sum += g[selected][i] as f64;
    }
    let mean = (sum / nf) as f32;
    values.iter_mut().for_each(|x| *x = -2.0);
    values[short[selected]] = mean.clamp(0.000001, 0.999999);
}

/// Run `f` over `jobs` on `workers` threads, results by index. Each calling
/// thread keeps its own pool, so the workers and their caches persist across
/// decisions, and concurrent callers do not share workers.
fn run_parallel<J: Sync, R: Send + Default + Clone>(jobs: &[J], workers: usize, f: impl Fn(&J) -> R + Sync) -> Vec<R> {
    use rayon::prelude::*;
    thread_local! {
        static POOL: std::cell::RefCell<Option<(usize, std::sync::Arc<rayon::ThreadPool>)>> = const { std::cell::RefCell::new(None) };
    }
    let w = workers.clamp(1, 8);
    let pool = POOL.with(|c| {
        let mut c = c.borrow_mut();
        match &*c {
            Some((n, p)) if *n == w => p.clone(),
            _ => {
                let p = std::sync::Arc::new(
                    rayon::ThreadPoolBuilder::new()
                        .num_threads(w)
                        .thread_name(|i| format!("rollout-{i}"))
                        .build()
                        .expect("rollout thread pool"),
                );
                *c = Some((w, p.clone()));
                p
            }
        }
    });
    pool.install(|| jobs.par_iter().with_max_len(1).map(&f).collect())
}
