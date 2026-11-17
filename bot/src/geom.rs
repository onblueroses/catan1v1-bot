//! The bot numbering of tiles, nodes and edges (BOT-SPEC 2.3) and the board
//! geometry derived from the engine topology in that numbering.
//!
//! Every tie-break and every floating-point sum of the bot follows this
//! numbering, so all loops over nodes, edges and tiles in this crate use it.

use catan_engine::topology::{EDGE_NODES, HEX_NODES, NODE_EDGES};
use std::sync::OnceLock;

/// Engine hex for each bot tile.
pub const TILE: [u8; 19] = [0, 1, 2, 6, 11, 15, 18, 17, 16, 12, 7, 3, 4, 5, 10, 14, 13, 8, 9];

/// Engine node for each bot node.
pub const NODE: [u8; 54] = [
    0, 4, 8, 12, 7, 3, 1, 5, 9, 13, 2, 6, 10, 14, 15, 20, 25, 19, 26, 32, 37, 31, 42, 46, 41, 36,
    50, 53, 49, 45, 40, 52, 48, 44, 39, 51, 47, 43, 28, 34, 38, 33, 16, 22, 27, 21, 17, 11, 18, 23,
    24, 30, 35, 29,
];

/// Engine edge for each bot edge.
pub const EDGE: [u8; 72] = [
    1, 7, 12, 11, 6, 0, 3, 8, 14, 13, 2, 5, 9, 16, 15, 4, 17, 22, 31, 30, 21, 32, 38, 48, 47, 37,
    53, 61, 60, 52, 46, 65, 71, 70, 64, 59, 58, 69, 68, 63, 57, 56, 67, 66, 62, 55, 41, 50, 54, 49,
    40, 24, 34, 39, 33, 23, 19, 25, 18, 10, 20, 27, 26, 29, 28, 45, 36, 51, 44, 43, 42, 35,
];

/// Number of dice pairs per total 2..=12, index `total - 2`.
pub const WAYS: [u8; 11] = catan_engine::dice::INITIAL_COUNTS;

/// `ways(n)` for a number token, 0 for the desert token 0.
#[inline]
pub fn ways(number: u8) -> u8 {
    if (2..=12).contains(&number) { WAYS[(number - 2) as usize] } else { 0 }
}

/// Fixed geometry in bot numbering.
pub struct Geometry {
    pub node_of_engine: [u8; 54],
    pub edge_of_engine: [u8; 72],
    pub tile_of_engine: [u8; 19],
    /// The six nodes of each tile.
    pub tile_nodes: [[u8; 6]; 19],
    /// Both nodes of each edge.
    pub edge_nodes: [[u8; 2]; 72],
    /// Incident edges of each node, ascending; the count is `node_edge_count`.
    pub node_edges: [[u8; 3]; 54],
    pub node_edge_count: [u8; 54],
    /// Tiles of each node, ascending.
    pub node_tiles: [[u8; 3]; 54],
    pub node_tile_count: [u8; 54],
    /// Neighbour mask of each node.
    pub node_neighbors: [u64; 54],
    /// Mask of the tile's nodes.
    pub tile_mask: [u64; 19],
}

pub fn geometry() -> &'static Geometry {
    static G: OnceLock<Geometry> = OnceLock::new();
    G.get_or_init(build)
}

fn build() -> Geometry {
    let mut node_of_engine = [0u8; 54];
    for (b, &e) in NODE.iter().enumerate() {
        node_of_engine[e as usize] = b as u8;
    }
    let mut edge_of_engine = [0u8; 72];
    for (b, &e) in EDGE.iter().enumerate() {
        edge_of_engine[e as usize] = b as u8;
    }
    let mut tile_of_engine = [0u8; 19];
    for (b, &e) in TILE.iter().enumerate() {
        tile_of_engine[e as usize] = b as u8;
    }
    let mut tile_nodes = [[0u8; 6]; 19];
    let mut tile_mask = [0u64; 19];
    for t in 0..19 {
        for (k, &n) in HEX_NODES[TILE[t] as usize].iter().enumerate() {
            let b = node_of_engine[n as usize];
            tile_nodes[t][k] = b;
            tile_mask[t] |= 1u64 << b;
        }
    }
    let mut edge_nodes = [[0u8; 2]; 72];
    for e in 0..72 {
        let [a, b] = EDGE_NODES[EDGE[e] as usize];
        edge_nodes[e] = [node_of_engine[a as usize], node_of_engine[b as usize]];
    }
    let mut node_edges = [[255u8; 3]; 54];
    let mut node_edge_count = [0u8; 54];
    let mut node_neighbors = [0u64; 54];
    for n in 0..54 {
        let mut list: Vec<u8> = NODE_EDGES[NODE[n] as usize]
            .iter()
            .filter(|&&e| e != 255)
            .map(|&e| edge_of_engine[e as usize])
            .collect();
        list.sort_unstable();
        node_edge_count[n] = list.len() as u8;
        for (k, &e) in list.iter().enumerate() {
            node_edges[n][k] = e;
            let [a, b] = edge_nodes[e as usize];
            let m = if a as usize == n { b } else { a };
            node_neighbors[n] |= 1u64 << m;
        }
    }
    let mut node_tiles = [[255u8; 3]; 54];
    let mut node_tile_count = [0u8; 54];
    for t in 0..19 {
        for &n in &tile_nodes[t] {
            let c = node_tile_count[n as usize] as usize;
            node_tiles[n as usize][c] = t as u8;
            node_tile_count[n as usize] += 1;
        }
    }
    Geometry {
        node_of_engine,
        edge_of_engine,
        tile_of_engine,
        tile_nodes,
        edge_nodes,
        node_edges,
        node_edge_count,
        node_tiles,
        node_tile_count,
        node_neighbors,
        tile_mask,
    }
}

impl Geometry {
    #[inline]
    pub fn edges_of(&self, n: usize) -> &[u8] {
        &self.node_edges[n][..self.node_edge_count[n] as usize]
    }
    #[inline]
    pub fn tiles_of(&self, n: usize) -> &[u8] {
        &self.node_tiles[n][..self.node_tile_count[n] as usize]
    }
}

/// Iterate the set bits of a node mask in ascending order.
#[inline]
pub fn bits64(mut m: u64) -> impl Iterator<Item = usize> {
    std::iter::from_fn(move || {
        if m == 0 {
            None
        } else {
            let b = m.trailing_zeros() as usize;
            m &= m - 1;
            Some(b)
        }
    })
}

/// Iterate the set bits of an edge mask in ascending order.
#[inline]
pub fn bits128(mut m: u128) -> impl Iterator<Item = usize> {
    std::iter::from_fn(move || {
        if m == 0 {
            None
        } else {
            let b = m.trailing_zeros() as usize;
            m &= m - 1;
            Some(b)
        }
    })
}
