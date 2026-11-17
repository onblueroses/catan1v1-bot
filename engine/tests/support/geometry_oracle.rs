//! Independent geometry oracle from the numbered edge list, without engine helpers.
use catan_engine::State;

pub const EDGES: [[usize; 2]; 72] = [
    [0, 3],
    [0, 4],
    [1, 4],
    [1, 5],
    [2, 5],
    [2, 6],
    [3, 7],
    [4, 8],
    [5, 9],
    [6, 10],
    [7, 11],
    [7, 12],
    [8, 12],
    [8, 13],
    [9, 13],
    [9, 14],
    [10, 14],
    [10, 15],
    [11, 16],
    [12, 17],
    [13, 18],
    [14, 19],
    [15, 20],
    [16, 21],
    [16, 22],
    [17, 22],
    [17, 23],
    [18, 23],
    [18, 24],
    [19, 24],
    [19, 25],
    [20, 25],
    [20, 26],
    [21, 27],
    [22, 28],
    [23, 29],
    [24, 30],
    [25, 31],
    [26, 32],
    [27, 33],
    [28, 33],
    [28, 34],
    [29, 34],
    [29, 35],
    [30, 35],
    [30, 36],
    [31, 36],
    [31, 37],
    [32, 37],
    [33, 38],
    [34, 39],
    [35, 40],
    [36, 41],
    [37, 42],
    [38, 43],
    [39, 43],
    [39, 44],
    [40, 44],
    [40, 45],
    [41, 45],
    [41, 46],
    [42, 46],
    [43, 47],
    [44, 48],
    [45, 49],
    [46, 50],
    [47, 51],
    [48, 51],
    [48, 52],
    [49, 52],
    [49, 53],
    [50, 53],
];
fn owner(building: u8) -> Option<usize> {
    (building != 0).then(|| (building as usize - 1) % 2)
}

pub fn assert_geometry(s: &State, edges: &[[usize; 2]]) {
    let mut distance = 0u64;
    for n in 0..54 {
        let clear = s.node_occupancy[n] == 0
            && edges
                .iter()
                .all(|ends| !ends.contains(&n) || s.node_occupancy[ends[0] + ends[1] - n] == 0);
        if clear {
            distance |= 1u64 << n;
        }
    }
    assert_eq!(s.cache.distance_sites, distance, "raw distance geometry");
    for p in 0..2 {
        let mut roads = 0u128;
        for (e, ends) in edges.iter().enumerate() {
            if s.edge_occupancy[e] == 0
                && ends.iter().any(|&n| match owner(s.node_occupancy[n]) {
                    Some(q) => q == p,
                    None => edges
                        .iter()
                        .enumerate()
                        .any(|(j, nodes)| nodes.contains(&n) && s.edge_occupancy[j] == p as u8 + 1),
                })
            {
                roads |= 1u128 << e;
            }
        }
        let mut settlements = 0u64;
        let mut cities = 0u64;
        for n in 0..54 {
            if s.node_occupancy[n] == p as u8 + 1 {
                cities |= 1u64 << n;
            }
            if distance & (1u64 << n) != 0
                && edges
                    .iter()
                    .enumerate()
                    .any(|(e, ends)| ends.contains(&n) && s.edge_occupancy[e] == p as u8 + 1)
            {
                settlements |= 1u64 << n;
            }
        }
        assert_eq!(
            s.cache.road_sites[p], roads,
            "raw road geometry, player {p}"
        );
        assert_eq!(
            s.cache.settlement_sites[p], settlements,
            "raw settlement geometry, player {p}"
        );
        assert_eq!(
            s.cache.city_sites[p], cities,
            "raw city geometry, player {p}"
        );
    }
}
