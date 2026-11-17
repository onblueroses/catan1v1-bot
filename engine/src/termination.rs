//! Conservative dead-position certificate. See ENCODING.md, E10.
use crate::{State, phase};

/// Return whether a valid, normalized state has frozen points below the target.
///
/// This omniscient predicate is separate from the win-only terminal test. False
/// does not imply that a win remains reachable. No state or hash field changes.
///
/// The body checks the seven conditions of ENCODING.md, "Dead positions", in
/// this order: 1 (setup complete, not terminal), 2 (both totals below the
/// target), 3 (no road can be placed: no piece left or no eligible edge),
/// 6 (empty development deck), 5 (no city upgrade: no piece left or no
/// settlement), 7 (Largest Army cannot change) and last 4 (no settlement
/// site). Conditions 3 to 5 read the cached site sets, which ignore cost.
/// The same file argues why these conditions freeze both point totals and
/// why every successor of a dead state is dead.
pub fn dead(state: &State) -> bool {
    if state.setup_pairs_done != 4
        || state.phase == phase::TERMINAL
        || state
            .cache
            .total_points
            .iter()
            .any(|&points| u16::from(points) >= state.config.victory_points)
        || (0..2).any(|p| state.cache.pieces_left[p][0] != 0 && state.cache.road_sites[p] != 0)
        || state.development_deck.iter().any(|&count| count != 0)
    {
        return false;
    }
    for p in 0..2 {
        if state.cache.pieces_left[p][2] != 0 && state.cache.city_sites[p] != 0 {
            return false;
        }
    }
    let knights = |p: usize| {
        u16::from(state.spent_development[p][0])
            + u16::from(state.playable_development[p][0])
            + u16::from(state.new_development[p][0])
    };
    if state.largest_army_holder < 2 {
        let holder = state.largest_army_holder as usize;
        if knights(1 - holder) > u16::from(state.spent_development[holder][0]) {
            return false;
        }
    } else if knights(0) >= 3 || knights(1) >= 3 {
        return false;
    }
    (state.cache.settlement_sites[0] | state.cache.settlement_sites[1]) == 0
}

impl State {
    /// Separate E10 certificate. The rules-engine terminal test remains win-only.
    pub fn is_dead(&self) -> bool {
        dead(self)
    }
}
