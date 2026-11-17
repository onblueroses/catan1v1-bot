//! Board facts in bot numbering, and reference board generation (BOT-SPEC 2.5).

use crate::geom::{NODE, TILE, geometry, ways};
use crate::rng::Rng;
use catan_engine::{Board, State};

/// Board data that the inputs and the sample seed read, in bot numbering.
#[derive(Clone, Debug)]
pub struct BoardInfo {
    /// Resource of each bot tile, 5 for the desert.
    pub res: [u8; 19],
    /// Number token of each bot tile, 0 for the desert.
    pub num: [u8; 19],
    /// Port kind per slot, 5 for generic.
    pub port_kind: [u8; 9],
    /// Port class of each node: 255 none, 0 generic, 1 + r for a 2:1 port.
    pub node_port: [u8; 54],
    /// pips(n): sum of ways over the node's tiles with a number.
    pub pips: [u32; 54],
}

impl BoardInfo {
    pub fn new(st: &State) -> BoardInfo {
        let g = geometry();
        let mut res = [0u8; 19];
        let mut num = [0u8; 19];
        for t in 0..19 {
            res[t] = st.board.terrain[TILE[t] as usize];
            num[t] = st.board.number[TILE[t] as usize];
        }
        let port_kind = st.board.port_type;
        let mut node_port = [255u8; 54];
        for (slot, pair) in st.board.port_nodes().iter().enumerate() {
            let kind = port_kind[slot];
            let class = if kind == 5 { 0 } else { 1 + kind };
            for &en in pair {
                node_port[g.node_of_engine[en as usize] as usize] = class;
            }
        }
        let mut pips = [0u32; 54];
        for n in 0..54 {
            for &t in g.tiles_of(n) {
                pips[n] += ways(num[t as usize]) as u32;
            }
        }
        BoardInfo { res, num, port_kind, node_port, pips }
    }
}

/// Reference port rings in bot node ids, per slot.
pub const PORT_RINGS: [[[u8; 2]; 9]; 2] = [
    [[18, 19], [22, 23], [28, 31], [35, 36], [40, 41], [42, 47], [5, 0], [6, 7], [12, 14]],
    [[1, 6], [10, 11], [14, 15], [20, 22], [26, 27], [31, 32], [37, 40], [44, 45], [47, 4]],
];

/// Generate a reference board from RNG `l`. Returns the engine board and the port ring.
pub fn generate_board(l: &mut Rng) -> (Board, u8) {
    let mut res: [u8; 19] = [0, 0, 0, 0, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 5];
    for i in (1..=18).rev() {
        let j = l.below(i as u64 + 1) as usize;
        res.swap(i, j);
    }
    let mut ports: [u8; 9] = [0, 1, 2, 3, 4, 5, 5, 5, 5];
    for i in (1..=8).rev() {
        let j = l.below(i as u64 + 1) as usize;
        ports.swap(i, j);
    }
    const TOKENS: [u8; 18] = [5, 2, 6, 3, 8, 10, 9, 12, 11, 4, 8, 10, 9, 4, 5, 6, 3, 11];
    let mut num = [0u8; 19];
    let mut k = 0;
    for t in 0..19 {
        if res[t] != 5 {
            num[t] = TOKENS[k];
            k += 1;
        }
    }
    let ring = if l.below(2) != 0 { 1 } else { 0 };
    let mut board = Board::default();
    for t in 0..19 {
        board.terrain[TILE[t] as usize] = res[t];
        board.number[TILE[t] as usize] = num[t];
    }
    board.port_type = ports;
    let nodes = PORT_RINGS[ring as usize].map(|[a, b]| [NODE[a as usize], NODE[b as usize]]);
    board.set_port_nodes(nodes).expect("reference port ring");
    (board, ring)
}
