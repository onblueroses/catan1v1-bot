//! Network file format, float evaluation and fixed-point evaluation (BOT-SPEC 4.1-4.3).

use crate::inputs::{Inputs, N_INPUTS};

/// Widest hidden layer supported by the stack buffers.
const MAX_WIDTH: usize = 1024;

pub struct Layer {
    pub n_in: usize,
    pub n_out: usize,
    /// Input-major weights: `w[i * n_out + o]`.
    pub w: Vec<f32>,
    pub b: Vec<f32>,
}

pub struct Net {
    pub w0: usize,
    pub layers: Vec<Layer>,
    /// Fixed-point first layer, input-major `wi[i * n1 + o]`.
    pub wi: Vec<i32>,
    pub bi: Vec<i32>,
    pub unit: f32,
    pub bound: f64,
    pub scale: f64,
    /// Process-unique id for the thread-local incremental state.
    id: usize,
    /// Compact first layer for the 320-wide kernel: `wi` clamped to i16, each
    /// 32-output segment of a row stored in the order of the 16-bit interleave
    /// (see `simd`), and the exact remainders `wi - w16` of the clamped weights
    /// as (output, remainder) lists per input row.
    w16: Vec<i16>,
    corr_off: Vec<u32>,
    corr: Vec<(u32, i32)>,
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

impl Net {
    /// Process-unique id of this loaded net.
    #[inline]
    pub fn id(&self) -> usize {
        self.id
    }

    pub fn load(path: &std::path::Path) -> Result<Net, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&bytes)
    }

    pub fn parse(b: &[u8]) -> Result<Net, String> {
        if b.len() < 8 || &b[..4] != b"CATN" {
            return Err("bad magic".into());
        }
        let l = u32_at(b, 4) as usize;
        if l == 0 || b.len() < 8 + 4 * (l + 1) {
            return Err("bad layer count".into());
        }
        let widths: Vec<usize> = (0..=l).map(|i| u32_at(b, 8 + 4 * i) as usize).collect();
        if ![175, 223, 329, 351].contains(&widths[0]) || widths[l] != 1 || widths[1..].iter().any(|&w| w > MAX_WIDTH) {
            return Err("bad widths".into());
        }
        let params: usize = (1..=l).map(|i| widths[i - 1] * widths[i] + widths[i]).sum();
        let header = 8 + 4 * (l + 1);
        if b.len() != header + 4 * params {
            return Err("bad length".into());
        }
        let mut off = header;
        let mut next = || {
            let x = f32::from_le_bytes(b[off..off + 4].try_into().unwrap());
            off += 4;
            x
        };
        let mut layers = Vec::new();
        for li in 1..=l {
            let (ni, no) = (widths[li - 1], widths[li]);
            let mut w = vec![0f32; ni * no];
            for o in 0..no {
                for i in 0..ni {
                    w[i * no + o] = next();
                }
            }
            let bias: Vec<f32> = (0..no).map(|_| next()).collect();
            if w.iter().chain(bias.iter()).any(|x| !x.is_finite()) {
                return Err("non-finite parameter".into());
            }
            layers.push(Layer { n_in: ni, n_out: no, w, b: bias });
        }
        // Fixed-point preparation.
        let first = &layers[0];
        let (ni, no) = (first.n_in, first.n_out);
        let mut bound_max = 0f64;
        for o in 0..no {
            let mut bound = (first.b[o].abs() * 100.0f32) as f64;
            for i in 0..ni {
                bound += (255.0f32 * first.w[i * no + o].abs()) as f64;
            }
            bound_max = bound_max.max(bound);
        }
        let bound = bound_max.max(1.0);
        let scale = ((1u64 << 30) as f64 / bound).log2().floor().exp2();
        let unit = (1.0f64 / (100.0 * scale)) as f32;
        let wi: Vec<i32> = first.w.iter().map(|&x| (x as f64 * scale).round() as i32).collect();
        let bi: Vec<i32> = first.b.iter().map(|&x| ((x * 100.0f32) as f64 * scale).round() as i32).collect();
        static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (mut w16, mut corr_off, mut corr) = (Vec::new(), Vec::new(), Vec::new());
        if no == 320 {
            w16 = vec![0i16; ni * 320];
            corr_off.push(0);
            for i in 0..ni {
                for o in 0..320 {
                    let w = wi[i * 320 + o];
                    let c = w.clamp(i16::MIN as i32, i16::MAX as i32);
                    if c != w {
                        corr.push((o as u32, w - c));
                    }
                    // Output 32j + 16h + 4c + t sits at word 8c + 4h + t of segment j.
                    let (j, r) = (o / 32, o % 32);
                    let (h, q) = (r / 16, r % 16);
                    w16[i * 320 + 32 * j + 8 * (q / 4) + 4 * h + q % 4] = c as i16;
                }
                corr_off.push(corr.len() as u32);
            }
        }
        Ok(Net { w0: widths[0], layers, wi, bi, unit, bound, scale, id, w16, corr_off, corr })
    }

    /// Steps 2-3 of 4.2 from first-layer activations (in `a`, length of the first layer).
    fn tail(&self, a0: &[f32]) -> f32 {
        let mut buf_a = [0f32; MAX_WIDTH];
        let mut buf_z = [0f32; MAX_WIDTH];
        let mut n = a0.len();
        buf_a[..n].copy_from_slice(a0);
        for layer in &self.layers[1..] {
            let no = layer.n_out;
            let z = &mut buf_z[..no];
            z.copy_from_slice(&layer.b);
            for i in 0..n {
                let ai = buf_a[i];
                if ai > 0.0 {
                    let row = &layer.w[i * no..(i + 1) * no];
                    for (zo, &w) in z.iter_mut().zip(row) {
                        *zo += ai * w;
                    }
                }
            }
            buf_a[..no].copy_from_slice(z);
            n = no;
        }
        let z = buf_a[0];
        1.0f32 / (1.0f32 + (-z).exp())
    }

    pub fn eval_float(&self, x: &Inputs) -> f32 {
        let first = &self.layers[0];
        let no = first.n_out;
        let mut a = [0f32; MAX_WIDTH];
        let a = &mut a[..no];
        a.copy_from_slice(&first.b);
        for i in 0..self.w0.min(N_INPUTS) {
            if x[i] != 0 {
                let s = x[i] as f32 * 0.01f32;
                let row = &first.w[i * no..(i + 1) * no];
                for (ao, &w) in a.iter_mut().zip(row) {
                    *ao += s * w;
                }
            }
        }
        self.tail(a)
    }

    /// The exact integer first-layer sums, written into `sum` (length of the first layer).
    pub fn fixed_sums_into(&self, x: &Inputs, sum: &mut [i32]) {
        let no = self.layers[0].n_out;
        sum.copy_from_slice(&self.bi);
        for i in 0..self.w0.min(N_INPUTS) {
            let xi = x[i] as i32;
            if xi != 0 {
                let row = &self.wi[i * no..(i + 1) * no];
                for (so, &w) in sum.iter_mut().zip(row) {
                    *so += xi * w;
                }
            }
        }
        debug_assert_eq!(sum.len(), no);
    }

    pub fn eval_fixed_from_sums(&self, sum: &[i32]) -> f32 {
        let mut a = [0f32; MAX_WIDTH];
        let a = &mut a[..sum.len()];
        for (ao, &s) in a.iter_mut().zip(sum) {
            *ao = s as f32 * self.unit;
        }
        self.tail(a)
    }

    pub fn eval_fixed(&self, x: &Inputs) -> f32 {
        // Integer sums are exact, so updating the last evaluated vector of this
        // thread by the changed inputs only gives the same sums (BOT-SPEC 4.3).
        let id = self.id;
        LAST.with(|cell| {
            let mut c = cell.borrow_mut();
            c.clock += 1;
            let clock = c.clock;
            let slots = &mut c.slots;
            // The nearest of the recent vectors of this net; the update goes into
            // the least recently used one, so the others stay available.
            let mut best: Option<(usize, u32)> = None;
            let mut lru: Option<(usize, u64)> = None;
            let mut count = 0;
            for (i, s) in slots.iter().enumerate() {
                if s.net != id {
                    continue;
                }
                count += 1;
                let d = count_diff(&s.x, x);
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((i, d));
                }
                if lru.is_none_or(|(_, st)| s.stamp < st) {
                    lru = Some((i, s.stamp));
                }
            }
            let target = match best {
                None => {
                    slots.push(LastEval { net: id, x: [0; X_PAD], sum: self.bi.clone(), stamp: 0 });
                    slots.len() - 1
                }
                Some((b, _)) if count < SLOTS => {
                    let copy = LastEval { net: id, x: slots[b].x, sum: slots[b].sum.clone(), stamp: 0 };
                    slots.push(copy);
                    slots.len() - 1
                }
                Some((b, _)) => {
                    let t = lru.unwrap().0;
                    if t != b {
                        let (src, dst) = if b < t {
                            let (l, r) = slots.split_at_mut(t);
                            (&l[b], &mut r[0])
                        } else {
                            let (l, r) = slots.split_at_mut(b);
                            (&r[0], &mut l[t])
                        };
                        dst.x = src.x;
                        dst.sum.copy_from_slice(&src.sum);
                    }
                    t
                }
            };
            let slot = &mut slots[target];
            slot.stamp = clock;
            #[cfg(all(target_arch = "x86_64", target_feature = "avx512f", target_feature = "avx512bw"))]
            if self.w0 == N_INPUTS && self.layers[0].n_out == 320 && self.layers.len() == 3 && self.layers[1].n_out == 32 {
                // SAFETY: the target features are enabled at compile time and the shapes are checked.
                return unsafe { simd::eval_320_32(self, x, slot) };
            }
            self.eval_fixed_generic(x, slot)
        })
    }

    fn eval_fixed_generic(&self, x: &Inputs, slot: &mut LastEval) -> f32 {
        let no = self.layers[0].n_out;
        let nnz = x.iter().filter(|&&b| b != 0).count();
        let ndiff = x.iter().zip(slot.x.iter()).filter(|(a, b)| a != b).count();
        if ndiff < nnz {
            for i in 0..self.w0.min(N_INPUTS) {
                let d = x[i] as i32 - slot.x[i] as i32;
                if d != 0 {
                    let row = &self.wi[i * no..(i + 1) * no];
                    for (so, &w) in slot.sum.iter_mut().zip(row) {
                        *so += d * w;
                    }
                }
            }
        } else {
            self.fixed_sums_into(x, &mut slot.sum);
        }
        slot.x[..N_INPUTS].copy_from_slice(x);
        self.eval_fixed_from_sums(&slot.sum)
    }
}

/// Inputs stored with zero padding to a multiple of 64 bytes.
const X_PAD: usize = 384;

struct LastEval {
    net: usize,
    x: [u8; X_PAD],
    sum: Vec<i32>,
    stamp: u64,
}

struct LastSet {
    clock: u64,
    slots: Vec<LastEval>,
}

thread_local! {
    static LAST: std::cell::RefCell<LastSet> = const { std::cell::RefCell::new(LastSet { clock: 0, slots: Vec::new() }) };
}

/// Recent vectors kept per net and thread. Measured on a replay of 224k real
/// evaluations: 1, 2, 4, 8 slots took 0.902, 0.901, 0.880, 0.873 us each.
const SLOTS: usize = 4;

/// Number of input bytes in which `a` differs from `x`.
#[inline]
fn count_diff(a: &[u8; X_PAD], x: &Inputs) -> u32 {
    let mut n = 0u32;
    for i in 0..N_INPUTS / 8 {
        let u = u64::from_le_bytes(a[8 * i..8 * i + 8].try_into().unwrap());
        let v = u64::from_le_bytes(x[8 * i..8 * i + 8].try_into().unwrap());
        let d = u ^ v;
        // One bit per non-zero byte.
        let m = ((d & 0x7f7f_7f7f_7f7f_7f7f) + 0x7f7f_7f7f_7f7f_7f7f | d) & 0x8080_8080_8080_8080;
        n += m.count_ones();
    }
    for i in (N_INPUTS / 8) * 8..N_INPUTS {
        n += (a[i] != x[i]) as u32;
    }
    n
}

#[cfg(all(target_arch = "x86_64", target_feature = "avx512f", target_feature = "avx512bw"))]
mod simd {
    //! AVX-512 kernels for the 351-320-32-1 nets. Integer sums are exact in any
    //! order (i32 adds wrap, and the true sums fit in i32); the float layers keep
    //! the scalar order of operations per output (one multiply, then one add,
    //! inputs ascending), so every value is the same as in the scalar code.
    //!
    //! First layer: two changed rows at a time. A 64-byte load holds 32 i16
    //! weights of one row; unpacking the words of two rows gives i32 lanes
    //! (w1, w2), and a 16-bit multiply-add with (d1, d2) gives w1 d1 + w2 d2.
    //! The stored word order makes the unpacked lanes land in output order.
    use super::{LastEval, Net, X_PAD};
    use crate::inputs::{Inputs, N_INPUTS};
    use std::arch::x86_64::*;
    use std::mem::MaybeUninit;

    #[inline(always)]
    unsafe fn madd(acc: __m512i, a: __m512i, d: __m512i) -> __m512i {
        unsafe {
            #[cfg(target_feature = "avx512vnni")]
            {
                _mm512_dpwssd_epi32(acc, a, d)
            }
            #[cfg(not(target_feature = "avx512vnni"))]
            {
                _mm512_add_epi32(acc, _mm512_madd_epi16(a, d))
            }
        }
    }

    /// Add sum_k d_k * w16[row_k] onto ten output vectors starting at segment `seg0`.
    #[inline(always)]
    unsafe fn accumulate_half(w16: *const i16, rows: &[(u32, i32)], base: *const i32, out: *mut i32, seg0: usize) {
        unsafe {
            let o = 32 * seg0;
            let mut a = [_mm512_setzero_si512(); 10];
            for k in 0..10 {
                a[k] = _mm512_loadu_si512(base.add(o + 16 * k) as *const _);
            }
            let mut a0 = a[0];
            let mut a1 = a[1];
            let mut a2 = a[2];
            let mut a3 = a[3];
            let mut a4 = a[4];
            let mut a5 = a[5];
            let mut a6 = a[6];
            let mut a7 = a[7];
            let mut a8 = a[8];
            let mut a9 = a[9];
            let mut k = 0;
            while k < rows.len() {
                let (i1, d1) = rows[k];
                let (i2, d2) = if k + 1 < rows.len() { rows[k + 1] } else { (i1, 0) };
                k += 2;
                let d = _mm512_set1_epi32((d1 as u16 as u32 | ((d2 as u16 as u32) << 16)) as i32);
                let r1 = w16.add(i1 as usize * 320 + o);
                let r2 = w16.add(i2 as usize * 320 + o);
                macro_rules! seg {
                    ($lo:ident, $hi:ident, $j:expr) => {
                        let x = _mm512_loadu_si512(r1.add(32 * $j) as *const _);
                        let y = _mm512_loadu_si512(r2.add(32 * $j) as *const _);
                        $lo = madd($lo, _mm512_unpacklo_epi16(x, y), d);
                        $hi = madd($hi, _mm512_unpackhi_epi16(x, y), d);
                    };
                }
                seg!(a0, a1, 0);
                seg!(a2, a3, 1);
                seg!(a4, a5, 2);
                seg!(a6, a7, 3);
                seg!(a8, a9, 4);
            }
            for (k, v) in [a0, a1, a2, a3, a4, a5, a6, a7, a8, a9].into_iter().enumerate() {
                _mm512_storeu_si512(out.add(o + 16 * k) as *mut _, v);
            }
        }
    }

    #[inline]
    #[target_feature(enable = "avx512f,avx512bw")]
    pub(super) unsafe fn eval_320_32(net: &Net, x: &Inputs, slot: &mut LastEval) -> f32 {
        unsafe {
            let mut xp = [0u8; X_PAD];
            xp[..N_INPUTS].copy_from_slice(x);
            // Changed and non-zero bytes, 64 per mask.
            let mut diff = [0u64; X_PAD / 64];
            let mut nz = [0u64; X_PAD / 64];
            let mut ndiff = 0u32;
            let mut nnz = 0u32;
            for c in 0..X_PAD / 64 {
                let a = _mm512_loadu_si512(xp.as_ptr().add(64 * c) as *const _);
                let b = _mm512_loadu_si512(slot.x.as_ptr().add(64 * c) as *const _);
                diff[c] = _mm512_cmpneq_epi8_mask(a, b);
                nz[c] = _mm512_test_epi8_mask(a, a);
                ndiff += diff[c].count_ones();
                nnz += nz[c].count_ones();
            }
            // The rows to add with their factors: the changed inputs by their
            // change onto the last sums, or all non-zero inputs onto the biases.
            let mut buf = [MaybeUninit::<(u32, i32)>::uninit(); N_INPUTS];
            let mut nrows = 0usize;
            let incremental = ndiff < nnz;
            let masks = if incremental { &diff } else { &nz };
            for c in 0..X_PAD / 64 {
                let mut m = masks[c];
                while m != 0 {
                    let i = 64 * c + m.trailing_zeros() as usize;
                    m &= m - 1;
                    let d = if incremental { xp[i] as i32 - slot.x[i] as i32 } else { xp[i] as i32 };
                    buf[nrows].write((i as u32, d));
                    nrows += 1;
                }
            }
            let rows = std::slice::from_raw_parts(buf.as_ptr() as *const (u32, i32), nrows);
            let base = if incremental { slot.sum.as_ptr() } else { net.bi.as_ptr() };
            let out = slot.sum.as_mut_ptr();
            accumulate_half(net.w16.as_ptr(), rows, base, out, 0);
            accumulate_half(net.w16.as_ptr(), rows, base, out, 5);
            for &(i, d) in rows {
                let (a, b) = (net.corr_off[i as usize] as usize, net.corr_off[i as usize + 1] as usize);
                for &(o, c) in &net.corr[a..b] {
                    let s = &mut slot.sum[o as usize];
                    *s = s.wrapping_add(d.wrapping_mul(c));
                }
            }
            slot.x = xp;

            // First-layer activations s * unit; keep the positive ones in order.
            let unit = _mm512_set1_ps(net.unit);
            let mut pos_a = [MaybeUninit::<f32>::uninit(); 320];
            let mut pos_i = [MaybeUninit::<i32>::uninit(); 320];
            let mut np = 0usize;
            let lane = _mm512_setr_epi32(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);
            for k in 0..20 {
                let s = _mm512_loadu_si512(slot.sum.as_ptr().add(16 * k) as *const _);
                let a = _mm512_mul_ps(_mm512_cvtepi32_ps(s), unit);
                let m = _mm512_cmp_ps_mask::<_CMP_GT_OQ>(a, _mm512_setzero_ps());
                _mm512_mask_compressstoreu_ps(pos_a.as_mut_ptr().add(np) as *mut _, m, a);
                let idx = _mm512_add_epi32(lane, _mm512_set1_epi32(16 * k as i32));
                _mm512_mask_compressstoreu_epi32(pos_i.as_mut_ptr().add(np) as *mut _, m, idx);
                np += m.count_ones() as usize;
            }
            let pos_a = std::slice::from_raw_parts(pos_a.as_ptr() as *const f32, np);
            let pos_i = std::slice::from_raw_parts(pos_i.as_ptr() as *const i32, np);
            // Second layer, 32 outputs in two vectors.
            let l1 = &net.layers[1];
            let w1 = l1.w.as_ptr();
            let mut z0 = _mm512_loadu_ps(l1.b.as_ptr());
            let mut z1 = _mm512_loadu_ps(l1.b.as_ptr().add(16));
            for j in 0..np {
                let a = _mm512_set1_ps(pos_a[j]);
                let row = w1.add(pos_i[j] as usize * 32);
                z0 = _mm512_add_ps(z0, _mm512_mul_ps(a, _mm512_loadu_ps(row)));
                z1 = _mm512_add_ps(z1, _mm512_mul_ps(a, _mm512_loadu_ps(row.add(16))));
            }
            let mut a2 = [0f32; 32];
            _mm512_storeu_ps(a2.as_mut_ptr(), z0);
            _mm512_storeu_ps(a2.as_mut_ptr().add(16), z1);
            // Output layer, scalar in input order.
            let l2 = &net.layers[2];
            let mut z = l2.b[0];
            for i in 0..32 {
                let ai = a2[i];
                if ai > 0.0 {
                    z += ai * l2.w[i];
                }
            }
            1.0f32 / (1.0f32 + (-z).exp())
        }
    }
}

/// The two networks of the bot.
pub struct Nets {
    pub main: Net,
    pub opening: Net,
}

impl Nets {
    pub fn load(dir: &std::path::Path) -> Result<Nets, String> {
        Ok(Nets {
            main: Net::load(&dir.join("a320c1l.bin"))?,
            opening: Net::load(&dir.join("s320b.bin"))?,
        })
    }
}
