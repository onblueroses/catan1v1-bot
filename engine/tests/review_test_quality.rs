//! A test-only mutation of outcome enumeration. The engine is unchanged.
use catan_engine::*;
#[path = "support/chance_oracle.rs"]
mod chance_oracle;
struct MissingSeven(State);
impl MissingSeven {
    fn outcomes(&self) -> Vec<Outcome> {
        let mut buffer = [Outcome::default(); 11];
        let n = self.0.chance_outcomes(&mut buffer);
        let mut result: Vec<_> = buffer[..n].iter().copied().filter(|o| o.id != 5).collect();
        let total = result.iter().map(|o| o.weight).sum();
        for o in &mut result {
            o.total = total;
        }
        result
    }
    fn apply(&self, id: u16) -> Result<State, Error> {
        // This repeats rules.rs apply_outcome's acceptance gate against the
        // mutated enumerator. Production apply_outcome is never modified.
        if !self.outcomes().iter().any(|o| o.id == id) {
            return Err(Error::InvalidOutcome);
        }
        self.0.apply_outcome(id)
    }
}
#[test]
fn e2_chance_legality_property_accepts_a_missing_positive_mass_outcome() {
    let mut s = State::new(Board::default(), Config::default()).unwrap();
    for (n, e) in [(8, 7), (47, 66), (50, 65), (14, 21)] {
        s = s.apply_action(79 + n).unwrap().apply_action(7 + e).unwrap();
    }
    let s = s.apply_action(0).unwrap();
    s.validate().unwrap();
    let mutant = MissingSeven(s);
    let outcomes = mutant.outcomes();
    // These are the chance assertions from properties.rs: normalization,
    // sorted positive masses, and apply acceptance equal to enumeration.
    let total = outcomes[0].total;
    assert!(total > 0);
    assert_eq!(outcomes.iter().map(|o| o.weight).sum::<u128>(), total);
    assert!(outcomes.iter().all(|o| o.weight > 0 && o.total == total));
    assert!(outcomes.windows(2).all(|w| w[0].id < w[1].id));
    for id in 0..21 {
        assert_eq!(
            mutant.apply(id).is_ok(),
            outcomes.iter().any(|o| o.id == id)
        );
    }
    // The supplied TypeScript's first roll has all eleven totals and assigns
    // probability 6/36 to seven. These constants do not use the engine's gate.
    let source_support: Vec<u16> = (0..11).collect();
    let mut control = [Outcome::default(); 11];
    let n = s.chance_outcomes(&mut control);
    assert_eq!(
        control[..n].iter().map(|o| o.id).collect::<Vec<_>>(),
        source_support
    );
    assert_eq!(control[5].weight * 36, control[5].total * 6);
    assert_ne!(
        outcomes.iter().map(|o| o.id).collect::<Vec<_>>(),
        source_support
    );
    chance_oracle::assert_outcomes(&s, &control[..n]);
    let rejected = std::panic::catch_unwind(|| {
        chance_oracle::assert_outcomes(&s, &outcomes);
    });
    assert!(
        rejected.is_err(),
        "new independent oracle must reject the mutation"
    );
    assert!(s.apply_outcome(5).is_ok());
    assert!(!outcomes.iter().any(|o| o.id == 5));
    println!(
        "E2: original chance-consistency assertions PASS after removing a legal seven; source first-roll and shared property oracles REJECT the mutation"
    );
}
