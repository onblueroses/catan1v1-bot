//! The 181-byte state key and the sample seeds (BOT-SPEC 8.1).

use crate::board::BoardInfo;
use crate::geom::geometry;
use crate::inputs::{Hidden, Pieces};
use crate::position::{Pos, bp};

pub fn key181(pos: &Pos, d: u8) -> [u8; 181] {
    let g = geometry();
    let st = &pos.st;
    let pc = Pieces::of(pos);
    let mut k = [0u8; 181];
    for p in 0..2 {
        let b = 58 * p;
        for r in 0..5 {
            k[b + r] = st.resources[p][r];
            k[b + 5 + r] = st.playable_development[p][r] + st.new_development[p][r];
            k[b + 10 + r] = st.new_development[p][r];
            k[b + 15 + r] = st.spent_development[p][r];
            k[b + 20 + r] = st.cache.trade_rates[p][r];
        }
        k[b + 25] = (st.turn_owner as usize == p && st.development_played) as u8;
        k[b + 26..b + 34].copy_from_slice(&pc.settle[p].to_le_bytes());
        k[b + 34..b + 42].copy_from_slice(&pc.city[p].to_le_bytes());
        k[b + 42..b + 58].copy_from_slice(&pc.roads[p].to_le_bytes());
    }
    k[116..121].copy_from_slice(&st.bank);
    k[121..126].copy_from_slice(&st.development_deck);
    k[126] = pos.mover;
    k[127] = st.turn_owner;
    k[128] = pos.phase;
    let t = if st.turn_number == 0 { 0 } else { st.turn_number - 1 };
    k[129..133].copy_from_slice(&(t as u32).to_le_bytes());
    k[133..137].copy_from_slice(&pos.step.to_le_bytes());
    k[137] = pos.setup_placed;
    k[138] = g.tile_of_engine[st.robber_hex as usize];
    k[139] = pos.free_roads_left;
    k[140] = pos.last_total;
    k[141] = st.cache.road_lengths[0];
    k[142] = st.cache.road_lengths[1];
    k[143] = if pos.phase == bp::SETUP_ROAD && st.setup_anchor < 54 {
        g.node_of_engine[st.setup_anchor as usize]
    } else {
        255
    };
    k[144] = st.longest_road_holder;
    k[145] = st.largest_army_holder;
    k[146] = st.winner;
    k[147] = pos.before_roll as u8;
    k[148] = pos.discard_active;
    k[149] = pos.discard_slots[0][0];
    k[150] = pos.discard_slots[0][1];
    k[151] = pos.discard_slots[1][0];
    k[152] = pos.discard_slots[1][1];
    k[153..164].copy_from_slice(&pos.dice_counts);
    k[164..169].copy_from_slice(&st.dice.recent);
    k[169] = st.dice.recent_len;
    k[170..172].copy_from_slice(&(st.dice.sevens[0] as u16).to_le_bytes());
    k[172..174].copy_from_slice(&(st.dice.sevens[1] as u16).to_le_bytes());
    k[174] = st.dice.streak_owner;
    k[175..177].copy_from_slice(&(st.dice.streak_length as u16).to_le_bytes());
    k[177..181].copy_from_slice(&(d as u32).to_le_bytes());
    k
}

const FNV_PRIME: u64 = 0x100000001b3;

/// Root seed of a root viewpoint.
pub fn root_seed(key: &[u8; 181], v: u8, hid: Hidden, bi: &BoardInfo, port_ring: u8) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for w in 0..23 {
        let mut word = [0u8; 8];
        let lo = 8 * w;
        let hi = (lo + 8).min(181);
        word[..hi - lo].copy_from_slice(&key[lo..hi]);
        h = (h ^ u64::from_le_bytes(word)).wrapping_mul(FNV_PRIME);
    }
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51afd7ed558ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ceb9fe1a85ec53);
    h ^= h >> 33;
    let mut extra: Vec<u8> = vec![v, hid.h, hid.n, hid.d];
    for t in 0..19 {
        extra.push(if bi.res[t] == 5 { 255 } else { bi.res[t] });
        extra.push(bi.num[t]);
    }
    for s in 0..9 {
        extra.push(if bi.port_kind[s] == 5 { 255 } else { bi.port_kind[s] });
    }
    extra.push(port_ring);
    for b in extra {
        h = (h ^ b as u64).wrapping_mul(FNV_PRIME);
    }
    h
}

pub fn sample_seed(root: u64, i: usize) -> u64 {
    root ^ 0x9e3779b97f4a7c15u64.wrapping_mul(i as u64 + 1)
}
