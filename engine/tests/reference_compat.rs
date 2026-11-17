use catan_engine::{Board, Config, State, phase};

#[test]
fn custom_ports_are_board_data_and_roundtrip_without_v1_aliasing() {
    let mut b = Board::default();
    b.set_port_nodes([
        [1, 4],
        [2, 6],
        [15, 20],
        [37, 42],
        [50, 53],
        [48, 52],
        [38, 43],
        [21, 27],
        [7, 11],
    ])
    .unwrap();
    b.validate().unwrap();
    let s = State::new(b, Config::default())
        .unwrap()
        .apply_action(83)
        .unwrap();
    assert_eq!(s.cache.trade_rates[0][0], 2);
    assert_eq!(
        State::from_compatibility_bytes(&s.compatibility_bytes()).unwrap(),
        s
    );
    let mut other = s;
    other.board.port_edges = Board::default_port_edges();
    other.recompute_caches();
    assert_ne!(s.hash(), other.hash());
    assert_ne!(s.cache.trade_rates, other.cache.trade_rates);
}

#[test]
fn reference_rounding_uses_integer_hundredths_and_preserves_default_model() {
    let mut s = State::new(Board::default(), Config::default()).unwrap();
    s.phase = phase::DICE;
    s.dice.initialized_player_mask = 3;
    s.dice.sevens = [5, 8];
    s.dice.streak_owner = 0;
    s.dice.streak_length = 3;
    let default = s.dice_probabilities();
    s.config.reference_dice_rounding = true;
    let reference = s.dice_probabilities();
    assert_eq!(reference.weights[5], 6 * 100 * 4);
    assert_ne!(
        reference.weights[5] * default.total,
        default.weights[5] * reference.total
    );
    s.config.reference_dice_rounding = false;
    assert_eq!(s.dice_probabilities(), default);
}

#[test]
fn invalid_port_geometry_is_rejected() {
    let mut b = Board::default();
    b.port_edges[1] = b.port_edges[0];
    assert!(b.validate().is_err());
    b = Board::default();
    b.port_edges[0] = 72;
    assert!(b.validate().is_err());
}
