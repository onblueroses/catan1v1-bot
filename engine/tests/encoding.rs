//! E7, D5: the configuration flag `road_building_needs_legal_edge` sits in
//! canonical byte 21, and the worked hashes of ENCODING.md are unchanged.
use catan_engine::{Board, Config, State};

#[test]
fn d5_config_uses_byte_21_and_preserves_default_worked_hashes() {
    let default = State::new(Board::default(), Config::default()).unwrap();
    assert!(!default.config.road_building_needs_legal_edge);
    assert_eq!(&default.canonical_bytes()[21..24], &[0, 0, 0]);
    assert_eq!(default.hash(), 0x7a97e8f83c5198b0);
    let settlement = default.apply_action(79).unwrap();
    assert_eq!(settlement.hash(), 0xc8d1887e91a81959);
    assert_eq!(
        settlement.apply_action(7).unwrap().hash(),
        0x53323333dfad046a
    );

    let mut enabled = default;
    enabled.config.road_building_needs_legal_edge = true;
    let bytes = enabled.canonical_bytes();
    assert_eq!(&bytes[21..24], &[1, 0, 0]);
    let mut expected = default.canonical_bytes();
    expected[21] = 1;
    assert_eq!(bytes, expected);
    assert_ne!(enabled.hash(), default.hash());
    assert_eq!(State::from_canonical_bytes(&bytes).unwrap(), enabled);
}

#[test]
fn d5_config_rejects_nonboolean_wire_values_and_reserved_padding() {
    let bytes = State::new(Board::default(), Config::default())
        .unwrap()
        .canonical_bytes();
    for value in 2..=u8::MAX {
        let mut bad = bytes;
        bad[21] = value;
        assert!(State::from_canonical_bytes(&bad).is_err(), "byte21={value}");
    }
    for offset in [22, 23] {
        let mut bad = bytes;
        bad[offset] = 1;
        assert!(
            State::from_canonical_bytes(&bad).is_err(),
            "padding {offset}"
        );
    }
}

#[test]
fn d5_config_defaults_old_json_to_false_and_serializes_explicitly() {
    let mut old = serde_json::to_value(Config::default()).unwrap();
    assert_eq!(old["road_building_needs_legal_edge"], false);
    old.as_object_mut()
        .unwrap()
        .remove("road_building_needs_legal_edge");
    let config: Config = serde_json::from_value(old.clone()).unwrap();
    assert!(!config.road_building_needs_legal_edge);
    old["road_building_needs_legal_edge"] = true.into();
    assert!(
        serde_json::from_value::<Config>(old.clone())
            .unwrap()
            .road_building_needs_legal_edge
    );
    old["road_building_needs_legal_edge"] = 1.into();
    assert!(serde_json::from_value::<Config>(old).is_err());
}
