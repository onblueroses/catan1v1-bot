//! B1: every embedded adjacency table equals `../spec/topology.json`. The
//! file is included at compile time, so this test needs it at that path.
use catan_engine::topology::*;
use serde_json::Value;
#[test]
fn b1_all_embedded_adjacencies_match_the_supplied_graph() {
    let graph: Value = serde_json::from_str(include_str!("../../spec/topology.json")).unwrap();
    fn ids(value: &Value) -> Vec<u8> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u8)
            .collect()
    }
    fn unpad(a: &[u8]) -> Vec<u8> {
        a.iter().copied().filter(|&n| n != 255).collect()
    }
    assert_eq!(graph["nodes"].as_array().unwrap().len(), 54);
    assert_eq!(graph["edges"].as_array().unwrap().len(), 72);
    for h in 0..19 {
        assert_eq!(HEX_NODES[h].to_vec(), ids(&graph["hexes"][h]["nodes"]));
        assert_eq!(HEX_EDGES[h].to_vec(), ids(&graph["hexes"][h]["edges"]));
        let expected: Vec<_> = (0..19)
            .filter(|&k| {
                k != h
                    && HEX_NODES[h]
                        .iter()
                        .filter(|n| HEX_NODES[k].contains(n))
                        .count()
                        == 2
            })
            .map(|n| n as u8)
            .collect();
        assert_eq!(unpad(&HEX_NEIGHBORS[h]), expected);
    }
    for n in 0..54 {
        assert_eq!(unpad(&NODE_EDGES[n]), ids(&graph["nodes"][n]["edges"]));
        assert_eq!(unpad(&NODE_HEXES[n]), ids(&graph["nodes"][n]["hexes"]));
        assert_eq!(
            unpad(&NODE_NEIGHBORS[n]),
            ids(&graph["nodes"][n]["neighbors"])
        );
    }
    for e in 0..72 {
        assert_eq!(
            EDGE_NODES[e],
            [
                graph["edges"][e]["a"].as_u64().unwrap() as u8,
                graph["edges"][e]["b"].as_u64().unwrap() as u8
            ]
        );
    }
    for p in 0..9 {
        assert_eq!(
            PORT_NODES[p].to_vec(),
            ids(&graph["port_slots"][p]["nodes"])
        );
    }
}
