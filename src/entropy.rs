//! Randomness model.

use crate::battle::{Battle, BattleRules};
use crate::error::WeaselResult;
use crate::event::{Event, EventKind, EventProcessor, EventQueue, EventTrigger};
use num_traits::Num;
#[cfg(feature = "serialization")]
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::cmp::Ordering;
use std::fmt::Debug;

/// Manages everything related to randomness inside a battle.
pub struct Entropy<R: BattleRules> {
    model: EntropyModel<R>,
    rules: R::ER,
}

impl<R: BattleRules> Entropy<R> {
    /// Creates a new entropy object.
    pub(crate) fn new(seed: Option<EntropySeed<R>>, rules: R::ER) -> Entropy<R> {
        Entropy {
            model: rules.generate_model(&seed),
            rules,
        }
    }

    /// See [generate](EntropyRules::generate).
    pub fn generate(&mut self, low: EntropyOutput<R>, high: EntropyOutput<R>) -> EntropyOutput<R> {
        match low.partial_cmp(&high) {
            Some(Ordering::Less) => self.rules.generate(&mut self.model, low, high),
            Some(Ordering::Greater) => self.rules.generate(&mut self.model, high, low),
            Some(Ordering::Equal) => low,
            None => panic!("incomparable range! low: {:?}, high: {:?}", low, high),
        }
    }

    /// Returns the entropy model. It contains all data starting from which `EntropyRules`
    /// can compute entropy for this battle.
    pub fn model(&self) -> &EntropyModel<R> {
        &self.model
    }

    /// Returns a mutable reference to the entropy model.
    pub fn model_mut(&mut self) -> &mut EntropyModel<R> {
        &mut self.model
    }

    /// Returns the `EntropyRules` in use.
    pub fn rules(&self) -> &R::ER {
        &self.rules
    }

    /// Returns a mutable reference to the `EntropyRules` in use.
    pub fn rules_mut(&mut self) -> &mut R::ER {
        &mut self.rules
    }

    /// Regenerates this entropy's model starting from the given seed.
    pub(crate) fn regenerate_model(&mut self, seed: &Option<EntropySeed<R>>) {
        self.model = self.rules.generate_model(seed)
    }
}

/// Defines how casuality works inside the battle system.
///
/// Entropy must be deterministic. If you use a generator, make sure that by starting
/// from an identical seed the same sequence of random bits will be reproduced.
pub trait EntropyRules {
    #[cfg(not(feature = "serialization"))]
    /// See [EntropySeed](type.EntropySeed.html).
    type EntropySeed: Clone + Debug;
    #[cfg(feature = "serialization")]
    /// See [EntropySeed](type.EntropySeed.html).
    type EntropySeed: Clone + Debug + Serialize + for<'a> Deserialize<'a>;

    /// See [EntropyModel](type.EntropyModel.html).
    type EntropyModel;
    /// See [EntropyOutput](type.EntropyOutput.html).
    type EntropyOutput: PartialOrd + Copy + Num + Debug;

    /// Generates an `EntropyModel` starting from an `EntropySeed`.
    fn generate_model(&self, seed: &Option<Self::EntropySeed>) -> Self::EntropyModel;

    /// Generates a random value within a half-open range [`low`, `high`).
    ///
    /// `high` is guaranteed to be greater or equal to `low`.
    fn generate(
        &self,
        model: &mut Self::EntropyModel,
        low: Self::EntropyOutput,
        high: Self::EntropyOutput,
    ) -> Self::EntropyOutput;
}

/// Type to represent an entropy seed.
/// It is used to bootstrap the `EntropyModel` for a game.
pub type EntropySeed<R> = <<R as BattleRules>::ER as EntropyRules>::EntropySeed;

/// Type to store all information about the entropy in the game.
///
/// The entropy model is the source of randomness (or the lack thereof) for a battle.\
/// For example, it can be a pseudo random number generator.
pub type EntropyModel<R> = <<R as BattleRules>::ER as EntropyRules>::EntropyModel;

/// The exact type of the random numbers generated by the entropy rules.
pub type EntropyOutput<R> = <<R as BattleRules>::ER as EntropyRules>::EntropyOutput;

/// Event to reset the entropy model.
///
/// # Examples
/// ```
/// use weasel::battle::{Battle, BattleRules};
/// use weasel::entropy::ResetEntropy;
/// use weasel::event::{EventTrigger, EventKind};
/// use weasel::{Server, battle_rules, rules::empty::*};
///
/// battle_rules! {}
///
/// let battle = Battle::builder(CustomRules::new()).build();
/// let mut server = Server::builder(battle).build();
///
/// ResetEntropy::trigger(&mut server).fire().unwrap();
/// assert_eq!(
///     server.battle().history().events()[0].kind(),
///     EventKind::ResetEntropy
/// );
/// ```
#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct ResetEntropy<R: BattleRules> {
    #[cfg_attr(
        feature = "serialization",
        serde(bound(
            serialize = "Option<EntropySeed<R>>: Serialize",
            deserialize = "Option<EntropySeed<R>>: Deserialize<'de>"
        ))
    )]
    seed: Option<EntropySeed<R>>,
}

impl<R: BattleRules> ResetEntropy<R> {
    /// Returns a trigger for this event.
    pub fn trigger<P: EventProcessor<R>>(processor: &mut P) -> ResetEntropyTrigger<R, P> {
        ResetEntropyTrigger {
            processor,
            seed: None,
        }
    }

    /// Returns the new seed.
    pub fn seed(&self) -> &Option<EntropySeed<R>> {
        &self.seed
    }
}

impl<R: BattleRules> std::fmt::Debug for ResetEntropy<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ResetEntropy {{ seed: {:?} }}", self.seed)
    }
}

impl<R: BattleRules> Clone for ResetEntropy<R> {
    fn clone(&self) -> Self {
        ResetEntropy {
            seed: self.seed.clone(),
        }
    }
}

impl<R: BattleRules + 'static> Event<R> for ResetEntropy<R> {
    fn verify(&self, _battle: &Battle<R>) -> WeaselResult<(), R> {
        Ok(())
    }

    fn apply(&self, battle: &mut Battle<R>, _: &mut Option<EventQueue<R>>) {
        battle.entropy.regenerate_model(&self.seed);
    }

    fn kind(&self) -> EventKind {
        EventKind::ResetEntropy
    }

    fn box_clone(&self) -> Box<dyn Event<R>> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Trigger to build and fire a `ResetEntropy` event.
pub struct ResetEntropyTrigger<'a, R, P>
where
    R: BattleRules,
    P: EventProcessor<R>,
{
    processor: &'a mut P,
    seed: Option<EntropySeed<R>>,
}

impl<'a, R, P> ResetEntropyTrigger<'a, R, P>
where
    R: BattleRules + 'static,
    P: EventProcessor<R>,
{
    /// Adds a seed to drive the generation of the new entropy model.
    pub fn seed(&'a mut self, seed: EntropySeed<R>) -> &'a mut ResetEntropyTrigger<'a, R, P> {
        self.seed = Some(seed);
        self
    }
}

impl<'a, R, P> EventTrigger<'a, R, P> for ResetEntropyTrigger<'a, R, P>
where
    R: BattleRules + 'static,
    P: EventProcessor<R>,
{
    fn processor(&'a mut self) -> &'a mut P {
        self.processor
    }

    /// Returns a `ResetEntropy` event.
    fn event(&self) -> Box<dyn Event<R>> {
        Box::new(ResetEntropy {
            seed: self.seed.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::Battle;
    use crate::battle_rules_with_entropy;
    use crate::server::Server;
    use crate::util::tests::server;
    use crate::{battle_rules, rules::empty::*};

    const DEFAULT_SEED: i32 = 3;

    #[derive(Debug, Default, Clone, Copy)]
    pub struct CustomEntropyRules {}

    impl EntropyRules for CustomEntropyRules {
        type EntropySeed = i32;
        type EntropyModel = i32;
        type EntropyOutput = i32;

        fn generate_model(&self, seed: &Option<Self::EntropySeed>) -> Self::EntropyModel {
            seed.unwrap_or(DEFAULT_SEED)
        }

        fn generate(
            &self,
            model: &mut Self::EntropyModel,
            _low: Self::EntropyOutput,
            _high: Self::EntropyOutput,
        ) -> Self::EntropyOutput {
            let model = *model;
            let result: Self::EntropyOutput = model;
            result
        }
    }

    battle_rules_with_entropy! { CustomEntropyRules }

    #[test]
    fn reset_model() {
        // Create a battle with a given entropy rules (always return seed).
        let battle = Battle::builder(CustomRules::new()).build();
        let mut server = Server::builder(battle).build();
        // Check entropy.
        assert_eq!(server.battle.entropy.generate(1, 5), DEFAULT_SEED);
        // Reset entropy.
        assert!(ResetEntropy::trigger(&mut server).seed(5).fire().is_ok());
        // Check entropy changed.
        assert_eq!(server.battle.entropy.generate(1, 5), 5);
    }

    #[test]
    fn low_high_guarantee() {
        battle_rules! {}
        let mut server = server(CustomRules::new());
        assert_eq!(server.battle.entropy.generate(5, 1), 3);
    }

    #[cfg(feature = "random")]
    #[test]
    fn empty_range() {
        // Check that having low == high works as expected.
        // Some distributions don't support it, so we prevent this situation in Entropy.
        battle_rules_with_entropy! { crate::rules::entropy::UniformDistribution<i32> }
        let mut server = server(CustomRules::new());
        assert_eq!(server.battle.entropy.generate(1, 1), 1);
    }
}
