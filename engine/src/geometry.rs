//! Public board geometry for caller-buffer action enumeration.
//!
//! The four site sets in `Cache` say where a piece could go if phase, stock
//! and cost allowed it: `distance_sites` (S2), `road_sites` (U3),
//! `settlement_sites` (U4) and `city_sites` (U5). `legal_actions` reads them,
//! and so does the dead-position predicate (E10). A placement changes only
//! the sites near it, so each transition calls one of the local updates here.
//! `recompute_geometry` is the board-wide reference that `validate` compares
//! against.
use crate::{NONE, State, topology::*};

impl State {
    /// Independent board-wide rebuild. Do not share local insertion helpers:
    /// equality with this result checks the incremental update boundary.
    pub(crate) fn recompute_geometry(&mut self) {
        self.cache.distance_sites = 0;
        self.cache.road_sites = [0; 2];
        self.cache.settlement_sites = [0; 2];
        self.cache.city_sites = [0; 2];
        for n in 0..54 {
            let x = self.node_occupancy[n];
            if x == 1 || x == 2 {
                self.cache.city_sites[(x - 1) as usize] |= 1u64 << n;
            }
            let clear = x == 0
                && NODE_NEIGHBORS[n]
                    .iter()
                    .all(|&v| v == NONE || self.node_occupancy[v as usize] == 0);
            if clear {
                self.cache.distance_sites |= 1u64 << n;
                for p in 0..2 {
                    if NODE_EDGES[n]
                        .iter()
                        .any(|&e| e != NONE && self.edge_occupancy[e as usize] == p as u8 + 1)
                    {
                        self.cache.settlement_sites[p] |= 1u64 << n;
                    }
                }
            }
        }
        for (e, nodes) in EDGE_NODES.iter().enumerate() {
            if self.edge_occupancy[e] != 0 {
                continue;
            }
            for p in 0..2 {
                if nodes.iter().any(|&n| {
                    let x = self.node_occupancy[n as usize];
                    if x != 0 {
                        (x - 1) % 2 == p as u8
                    } else {
                        NODE_EDGES[n as usize]
                            .iter()
                            .any(|&j| j != NONE && self.edge_occupancy[j as usize] == p as u8 + 1)
                    }
                }) {
                    self.cache.road_sites[p] |= 1u128 << e;
                }
            }
        }
    }

    /// Recompute one edge's membership in both players' road sites (U3).
    fn update_road_site(&mut self, edge: usize) {
        let bit = 1u128 << edge;
        for p in 0..2 {
            let connected = self.edge_occupancy[edge] == 0
                && EDGE_NODES[edge].iter().any(|&n| {
                    let x = self.node_occupancy[n as usize];
                    if x == 0 {
                        NODE_EDGES[n as usize]
                            .iter()
                            .any(|&e| e != NONE && self.edge_occupancy[e as usize] == p as u8 + 1)
                    } else {
                        (x - 1) % 2 == p as u8
                    }
                });
            self.cache.road_sites[p] &= !bit;
            if connected {
                self.cache.road_sites[p] |= bit;
            }
        }
    }

    /// Call after writing or clearing the road on `edge`. A road can open new
    /// road sites and new settlement sites at its two endpoints.
    pub(crate) fn update_geometry_road(&mut self, edge: usize) {
        // Only the changed edge and edges at its endpoints can change.
        // Recompute membership from raw occupancy for insertion and removal.
        for &n in &EDGE_NODES[edge] {
            for &e in &NODE_EDGES[n as usize] {
                if e != NONE {
                    self.update_road_site(e as usize);
                }
            }
            let bit = 1u64 << n;
            for p in 0..2 {
                self.cache.settlement_sites[p] &= !bit;
                if self.cache.distance_sites & bit != 0
                    && NODE_EDGES[n as usize]
                        .iter()
                        .any(|&e| e != NONE && self.edge_occupancy[e as usize] == p as u8 + 1)
                {
                    self.cache.settlement_sites[p] |= bit;
                }
            }
        }
    }

    /// Call after the turn owner's settlement is written on `node`. The node
    /// and its neighbours stop being distance sites for both players (S2).
    pub(crate) fn update_geometry_settlement(&mut self, node: usize) {
        let mut excluded = 1u64 << node;
        for &n in &NODE_NEIGHBORS[node] {
            if n != NONE {
                excluded |= 1u64 << n;
            }
        }
        self.cache.distance_sites &= !excluded;
        for sites in &mut self.cache.settlement_sites {
            *sites &= !excluded;
        }
        let p = self.turn_owner as usize;
        self.cache.city_sites[p] |= 1u64 << node;
        // A new opponent building can remove connections through this node.
        for &e in &NODE_EDGES[node] {
            if e != NONE {
                self.update_road_site(e as usize);
            }
        }
    }

    /// A city replaces a settlement without changing any road connection.
    pub(crate) fn update_geometry_city(&mut self, node: usize) {
        let bit = 1u64 << node;
        for p in 0..2 {
            self.cache.city_sites[p] &= !bit;
            if self.node_occupancy[node] == p as u8 + 1 {
                self.cache.city_sites[p] |= bit;
            }
        }
    }

    /// Undo restored the raw node. Rebuild only the distance neighborhood and
    /// incident road candidates. This uses no additional undo snapshot.
    pub(crate) fn restore_geometry_settlement(&mut self, node: usize) {
        for n in std::iter::once(node as u8).chain(NODE_NEIGHBORS[node]) {
            if n == NONE {
                continue;
            }
            let n = n as usize;
            let bit = 1u64 << n;
            let clear = self.node_occupancy[n] == 0
                && NODE_NEIGHBORS[n]
                    .iter()
                    .all(|&v| v == NONE || self.node_occupancy[v as usize] == 0);
            self.cache.distance_sites &= !bit;
            if clear {
                self.cache.distance_sites |= bit;
            }
            for p in 0..2 {
                self.cache.settlement_sites[p] &= !bit;
                if clear
                    && NODE_EDGES[n]
                        .iter()
                        .any(|&e| e != NONE && self.edge_occupancy[e as usize] == p as u8 + 1)
                {
                    self.cache.settlement_sites[p] |= bit;
                }
            }
        }
        self.update_geometry_city(node);
        for &e in &NODE_EDGES[node] {
            if e != NONE {
                self.update_road_site(e as usize);
            }
        }
    }
}
