//! Inanimate objects.

use crate::battle::{Battle, BattleRules};
use crate::character::{Character, CharacterRules, Statistic, StatisticId, StatisticsSeed};
use crate::entity::{Entity, EntityId};
use crate::error::{WeaselError, WeaselResult};
use crate::event::{Event, EventKind, EventProcessor, EventQueue, EventTrigger};
use crate::metric::system::OBJECTS_CREATED;
use crate::space::{Position, PositionClaim};
use crate::util::{collect_from_iter, Id};
#[cfg(feature = "serialization")]
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter, Result};

/// Type to represent the id of objects.
pub type ObjectId<R> = <<R as BattleRules>::CR as CharacterRules<R>>::ObjectId;

type Statistics<R> = HashMap<
    <<<R as BattleRules>::CR as CharacterRules<R>>::Statistic as Id>::Id,
    <<R as BattleRules>::CR as CharacterRules<R>>::Statistic,
>;

/// An object is an inanimate entity.
///
/// Objects possess a position and a set of statistics, but they can't start a round
/// nor activate abilities.\
/// Objects aren't part of any team.
pub struct Object<R: BattleRules> {
    id: EntityId<R>,
    position: Position<R>,
    statistics: Statistics<R>,
}

impl<R: BattleRules> Id for Object<R> {
    type Id = ObjectId<R>;

    fn id(&self) -> &ObjectId<R> {
        if let EntityId::Object(id) = &self.id {
            id
        } else {
            panic!("constraint violated: object's id has a wrong type")
        }
    }
}

impl<R: BattleRules> Entity<R> for Object<R> {
    fn entity_id(&self) -> &EntityId<R> {
        &self.id
    }

    fn position(&self) -> &Position<R> {
        &self.position
    }

    fn set_position(&mut self, position: Position<R>) {
        self.position = position;
    }
}

impl<R: BattleRules> Character<R> for Object<R> {
    fn statistics<'a>(&'a self) -> Box<dyn Iterator<Item = &'a Statistic<R>> + 'a> {
        Box::new(self.statistics.values())
    }

    fn statistic(&self, id: &StatisticId<R>) -> Option<&Statistic<R>> {
        self.statistics.get(id)
    }

    fn statistic_mut(&mut self, id: &StatisticId<R>) -> Option<&mut Statistic<R>> {
        self.statistics.get_mut(id)
    }

    fn add_statistic(&mut self, statistic: Statistic<R>) -> Option<Statistic<R>> {
        self.statistics.insert(statistic.id().clone(), statistic)
    }

    fn remove_statistic(&mut self, id: &StatisticId<R>) -> Option<Statistic<R>> {
        self.statistics.remove(id)
    }
}

/// Event to create a new object.
///
/// # Examples
/// ```
/// use weasel::battle::{Battle, BattleRules};
/// use weasel::event::EventTrigger;
/// use weasel::object::CreateObject;
/// use weasel::{Server, battle_rules, rules::empty::*};
///
/// battle_rules! {}
///
/// let battle = Battle::builder(CustomRules::new()).build();
/// let mut server = Server::builder(battle).build();
///
/// let object_id = 1;
/// let position = ();
/// CreateObject::trigger(&mut server, object_id, position)
///     .fire()
///     .unwrap();
/// assert_eq!(server.battle().entities().objects().count(), 1);
/// ```
#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct CreateObject<R: BattleRules> {
    #[cfg_attr(
        feature = "serialization",
        serde(bound(
            serialize = "ObjectId<R>: Serialize",
            deserialize = "ObjectId<R>: Deserialize<'de>"
        ))
    )]
    id: ObjectId<R>,

    #[cfg_attr(
        feature = "serialization",
        serde(bound(
            serialize = "Position<R>: Serialize",
            deserialize = "Position<R>: Deserialize<'de>"
        ))
    )]
    position: Position<R>,

    #[cfg_attr(
        feature = "serialization",
        serde(bound(
            serialize = "Option<StatisticsSeed<R>>: Serialize",
            deserialize = "Option<StatisticsSeed<R>>: Deserialize<'de>"
        ))
    )]
    statistics_seed: Option<StatisticsSeed<R>>,
}

impl<R: BattleRules> Debug for CreateObject<R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(
            f,
            "CreateObject {{ id: {:?}, position: {:?}, \
             statistics_seed: {:?} }}",
            self.id, self.position, self.statistics_seed
        )
    }
}

impl<R: BattleRules> Clone for CreateObject<R> {
    fn clone(&self) -> Self {
        CreateObject {
            id: self.id.clone(),
            position: self.position.clone(),
            statistics_seed: self.statistics_seed.clone(),
        }
    }
}

impl<R: BattleRules> CreateObject<R> {
    /// Returns a trigger for this event.
    pub fn trigger<'a, P: EventProcessor<R>>(
        processor: &'a mut P,
        id: ObjectId<R>,
        position: Position<R>,
    ) -> CreateObjectTrigger<'a, R, P> {
        CreateObjectTrigger {
            processor,
            id,
            position,
            statistics_seed: None,
        }
    }

    /// Returns the id of the object to be created.
    pub fn id(&self) -> &ObjectId<R> {
        &self.id
    }

    /// Returns the position that the object will take.
    pub fn position(&self) -> &Position<R> {
        &self.position
    }

    /// Returns the seed to generate the object's statistics.
    pub fn statistics_seed(&self) -> &Option<StatisticsSeed<R>> {
        &self.statistics_seed
    }
}

impl<R: BattleRules + 'static> Event<R> for CreateObject<R> {
    fn verify(&self, battle: &Battle<R>) -> WeaselResult<(), R> {
        // Check id duplication.
        if battle.entities().object(&self.id).is_some() {
            return Err(WeaselError::DuplicatedObject(self.id.clone()));
        }
        // Check position.
        if !battle.space().check_move(
            PositionClaim::Spawn(&EntityId::Object(self.id.clone())),
            &self.position,
        ) {
            return Err(WeaselError::PositionError(None, self.position.clone()));
        }
        Ok(())
    }

    fn apply(&self, battle: &mut Battle<R>, _: &mut Option<EventQueue<R>>) {
        // Statistics' generation is influenced by the given statistics_seed, if present.
        let it = battle.rules.character_rules().generate_statistics(
            &self.statistics_seed,
            &mut battle.entropy,
            &mut battle.metrics.write_handle(),
        );
        let statistics = collect_from_iter(it);
        // Create the object.
        let object = Object {
            id: EntityId::Object(self.id.clone()),
            position: self.position.clone(),
            statistics,
        };
        // Take the position.
        battle.state.space.move_entity(
            PositionClaim::Spawn(&EntityId::Object(self.id.clone())),
            Some(&self.position),
            &mut battle.metrics.write_handle(),
        );
        // Add the object to the entities.
        battle.state.entities.add_object(object);
        // Update metrics.
        battle
            .metrics
            .write_handle()
            .add_system_u64(OBJECTS_CREATED, 1)
            .unwrap_or_else(|err| panic!("constraint violated: {:?}", err));
    }

    fn kind(&self) -> EventKind {
        EventKind::CreateObject
    }

    fn box_clone(&self) -> Box<dyn Event<R>> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Trigger to build and fire a `CreateObject` event.
pub struct CreateObjectTrigger<'a, R, P>
where
    R: BattleRules + 'static,
    P: EventProcessor<R>,
{
    processor: &'a mut P,
    id: ObjectId<R>,
    position: Position<R>,
    statistics_seed: Option<StatisticsSeed<R>>,
}

impl<'a, R, P> CreateObjectTrigger<'a, R, P>
where
    R: BattleRules + 'static,
    P: EventProcessor<R>,
{
    /// Adds a seed to drive the generation of this object's statistics.
    pub fn statistics_seed(
        &'a mut self,
        seed: StatisticsSeed<R>,
    ) -> &'a mut CreateObjectTrigger<'a, R, P> {
        self.statistics_seed = Some(seed);
        self
    }
}

impl<'a, R, P> EventTrigger<'a, R, P> for CreateObjectTrigger<'a, R, P>
where
    R: BattleRules + 'static,
    P: EventProcessor<R>,
{
    fn processor(&'a mut self) -> &'a mut P {
        self.processor
    }

    /// Returns a `CreateObject` event.
    fn event(&self) -> Box<dyn Event<R>> {
        Box::new(CreateObject {
            id: self.id.clone(),
            position: self.position.clone(),
            statistics_seed: self.statistics_seed.clone(),
        })
    }
}

/// Event to remove an object from the battle.
///
/// The position occupied by the object will be freed.
///
/// # Examples
/// ```
/// use weasel::battle::{Battle, BattleRules};
/// use weasel::event::EventTrigger;
/// use weasel::object::{CreateObject, RemoveObject};
/// use weasel::{Server, battle_rules, rules::empty::*};
///
/// battle_rules! {}
///
/// let battle = Battle::builder(CustomRules::new()).build();
/// let mut server = Server::builder(battle).build();
///
/// let object_id = 1;
/// let position = ();
/// CreateObject::trigger(&mut server, object_id, position)
///     .fire()
///     .unwrap();
///
/// RemoveObject::trigger(&mut server, object_id).fire().unwrap();
/// assert_eq!(server.battle().entities().objects().count(), 0);
/// ```
#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct RemoveObject<R: BattleRules> {
    #[cfg_attr(
        feature = "serialization",
        serde(bound(
            serialize = "ObjectId<R>: Serialize",
            deserialize = "ObjectId<R>: Deserialize<'de>"
        ))
    )]
    id: ObjectId<R>,
}

impl<R: BattleRules> RemoveObject<R> {
    /// Returns a trigger for this event.
    pub fn trigger<P: EventProcessor<R>>(
        processor: &mut P,
        id: ObjectId<R>,
    ) -> RemoveObjectTrigger<R, P> {
        RemoveObjectTrigger { processor, id }
    }

    /// Returns the id of the object to be removed.
    pub fn id(&self) -> &ObjectId<R> {
        &self.id
    }
}

impl<R: BattleRules> Debug for RemoveObject<R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "RemoveObject {{ id: {:?} }}", self.id)
    }
}

impl<R: BattleRules> Clone for RemoveObject<R> {
    fn clone(&self) -> Self {
        RemoveObject {
            id: self.id.clone(),
        }
    }
}

impl<R: BattleRules + 'static> Event<R> for RemoveObject<R> {
    fn verify(&self, battle: &Battle<R>) -> WeaselResult<(), R> {
        // Verify if the object exists.
        if battle.entities().object(&self.id).is_none() {
            return Err(WeaselError::ObjectNotFound(self.id.clone()));
        }
        Ok(())
    }

    fn apply(&self, battle: &mut Battle<R>, _: &mut Option<EventQueue<R>>) {
        // Remove the object.
        let object = battle
            .state
            .entities
            .remove_object(&self.id)
            .unwrap_or_else(|err| panic!("constraint violated: {:?}", err));
        // Free the position.
        battle.state.space.move_entity(
            PositionClaim::Movement(&object as &dyn Entity<R>),
            None,
            &mut battle.metrics.write_handle(),
        );
    }

    fn kind(&self) -> EventKind {
        EventKind::RemoveObject
    }

    fn box_clone(&self) -> Box<dyn Event<R>> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Trigger to build and fire a `RemoveObject` event.
pub struct RemoveObjectTrigger<'a, R, P>
where
    R: BattleRules,
    P: EventProcessor<R>,
{
    processor: &'a mut P,
    id: ObjectId<R>,
}

impl<'a, R, P> EventTrigger<'a, R, P> for RemoveObjectTrigger<'a, R, P>
where
    R: BattleRules + 'static,
    P: EventProcessor<R>,
{
    fn processor(&'a mut self) -> &'a mut P {
        self.processor
    }

    /// Returns a `RemoveObject` event.
    fn event(&self) -> Box<dyn Event<R>> {
        Box::new(RemoveObject {
            id: self.id.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::BattleRules;
    use crate::rules::statistic::SimpleStatistic;
    use crate::util::tests::{object, server};
    use crate::{battle_rules, battle_rules_with_character, rules::empty::*};

    #[derive(Default)]
    pub struct CustomCharacterRules {}

    impl<R: BattleRules> CharacterRules<R> for CustomCharacterRules {
        type CreatureId = ();
        type ObjectId = u32;
        type Statistic = SimpleStatistic<u32, u32>;
        type StatisticsSeed = ();
        type StatisticsAlteration = ();
    }

    #[test]
    fn mutable_statistics() {
        battle_rules_with_character! { CustomCharacterRules }
        // Create a battle.
        let mut server = server(CustomRules::new());
        object(&mut server, 1, ());
        let object = server.battle.state.entities.object_mut(&1).unwrap();
        assert!(object.statistic(&1).is_none());
        object.add_statistic(SimpleStatistic::new(1, 50));
        assert!(object.statistic(&1).is_some());
        object.statistic_mut(&1).unwrap().set_value(25);
        assert_eq!(object.statistic(&1).unwrap().value(), 25);
        object.remove_statistic(&1);
        assert!(object.statistic(&1).is_none());
    }
}