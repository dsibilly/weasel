//! Everything related to the battle's rounds.

use crate::actor::{Actor, ActorRules};
use crate::battle::{Battle, BattleRules, Checkpoint};
use crate::entity::{Entities, Entity, EntityId};
use crate::entropy::Entropy;
use crate::error::{WeaselError, WeaselResult};
use crate::event::{Event, EventKind, EventProcessor, EventQueue, EventRights, EventTrigger};
use crate::metric::{system::*, WriteMetrics};
use crate::space::Space;
use crate::status::update_statuses;
use indexmap::IndexSet;
#[cfg(feature = "serialization")]
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt::{Debug, Formatter, Result};
use std::hash::Hash;
use std::marker::PhantomData;

/// Manages the battle's rounds. The main purpose is to tell which actor(s) will act next.
pub struct Rounds<R: BattleRules> {
    state: RoundStateType<R>,
    model: RoundsModel<R>,
    rules: R::RR,
}

impl<R: BattleRules> Rounds<R> {
    pub(crate) fn new(seed: Option<RoundsSeed<R>>, rules: R::RR) -> Rounds<R> {
        Rounds {
            state: RoundState::Ready,
            model: rules.generate_model(&seed),
            rules,
        }
    }

    /// Returns the rounds model. It contains all data starting from which `RoundsRules`
    /// can compute the order of acting in this battle.
    pub fn model(&self) -> &RoundsModel<R> {
        &self.model
    }

    /// Returns a mutable reference to the rounds model.
    pub fn model_mut(&mut self) -> &mut RoundsModel<R> {
        &mut self.model
    }

    /// Returns true if the entity with the given id is among the current actors.
    /// Entity existence is not verified.
    pub fn is_acting(&self, entity_id: &EntityId<R>) -> bool {
        self.state.has_actor(entity_id)
    }

    /// See [eligible](trait.RoundsRules.html#method.eligible).
    fn eligible(&self, actor: &dyn Actor<R>) -> bool {
        self.rules.eligible(&self.model, actor)
    }

    /// Returns the state of the current round.
    pub fn state(&self) -> &RoundStateType<R> {
        &self.state
    }

    /// Sets the state of the current round.
    pub(crate) fn set_state(&mut self, state: RoundStateType<R>) {
        self.state = state;
    }

    /// Returns the `RoundRules` in use.
    pub fn rules(&self) -> &R::RR {
        &self.rules
    }

    /// Returns a mutable reference to the `RoundRules` in use.
    pub fn rules_mut(&mut self) -> &mut R::RR {
        &mut self.rules
    }

    /// Called when a new actor is added to the battle.
    pub(crate) fn on_actor_added(
        &mut self,
        actor: &dyn Actor<R>,
        entropy: &mut Entropy<R>,
        metrics: &mut WriteMetrics<R>,
    ) {
        self.rules
            .on_actor_added(&mut self.model, actor, entropy, metrics);
    }

    /// Called when an actor is removed from the battle.
    pub(crate) fn on_actor_removed(
        &mut self,
        actor: &dyn Actor<R>,
        entropy: &mut Entropy<R>,
        metrics: &mut WriteMetrics<R>,
    ) {
        self.rules
            .on_actor_removed(&mut self.model, actor, entropy, metrics);
    }

    /// Invoked when a round ends.
    pub(crate) fn on_end(
        &mut self,
        entities: &Entities<R>,
        space: &Space<R>,
        actor: &dyn Actor<R>,
        entropy: &mut Entropy<R>,
        metrics: &mut WriteMetrics<R>,
    ) {
        self.rules
            .on_end(entities, space, &mut self.model, actor, entropy, metrics);
    }

    /// Regenerates this rounds' model starting from the given seed.
    pub(crate) fn regenerate_model(&mut self, seed: &Option<RoundsSeed<R>>) {
        self.model = self.rules.generate_model(seed)
    }
}

/// `RoundState` alias parameterized on the `BattleRules` R.
pub type RoundStateType<R> = RoundState<EntityId<R>>;

/// State machine to manage the rounds' state.
#[derive(Debug, Clone, PartialEq)]
pub enum RoundState<EI>
where
    EI: Debug + Hash + Eq,
{
    /// A new round is ready to start.
    Ready,
    /// A round is in progress.
    Started(IndexSet<EI>),
}

impl<EI> RoundState<EI>
where
    EI: Debug + Hash + Eq,
{
    /// Returns true if the round state is `Started` and the entity is one of its actors.
    pub fn has_actor(&self, entity_id: &EI) -> bool {
        if let RoundState::Started(actors) = self {
            actors.contains(entity_id)
        } else {
            false
        }
    }
}

/// Rules to determine the order of rounds among actors.
///
/// These rules must provide to the battle system the information needed to know
/// if an actor can take an action in a given moment. In other words, they manage the
/// time dimension, which is by definition divided in *turns* (or *rounds*).
pub trait RoundsRules<R: BattleRules> {
    #[cfg(not(feature = "serialization"))]
    /// See [RoundsSeed](type.RoundsSeed.html).
    type RoundsSeed: Debug + Clone + Send;
    #[cfg(feature = "serialization")]
    /// See [RoundsSeed](type.RoundsSeed.html).
    type RoundsSeed: Debug + Clone + Send + Serialize + for<'a> Deserialize<'a>;

    /// See [RoundsModel](type.RoundsModel.html).
    type RoundsModel;

    /// Generates a `RoundsModel` starting from a `RoundsSeed`.
    fn generate_model(&self, seed: &Option<Self::RoundsSeed>) -> Self::RoundsModel;

    /// Returns whether the given actor is eligible to start a new round.
    ///
    /// The provided implementation accepts any actor.
    fn eligible(&self, _model: &Self::RoundsModel, _actor: &dyn Actor<R>) -> bool {
        true
    }

    /// Invoked when a new round begins.
    ///
    /// The provided implementation does nothing.
    fn on_start(
        &self,
        _entities: &Entities<R>,
        _space: &Space<R>,
        _model: &mut Self::RoundsModel,
        _actor: &dyn Actor<R>,
        _entropy: &mut Entropy<R>,
        _metrics: &mut WriteMetrics<R>,
    ) {
    }

    /// Invoked when the current round ends.
    ///
    /// The provided implementation does nothing.
    fn on_end(
        &self,
        _entities: &Entities<R>,
        _space: &Space<R>,
        _model: &mut Self::RoundsModel,
        _actor: &dyn Actor<R>,
        _entropy: &mut Entropy<R>,
        _metrics: &mut WriteMetrics<R>,
    ) {
    }

    /// Invoked when a new actor is added to the battle.
    ///
    /// The provided implementation does nothing.
    fn on_actor_added(
        &self,
        _model: &mut Self::RoundsModel,
        _actor: &dyn Actor<R>,
        _entropy: &mut Entropy<R>,
        _metrics: &mut WriteMetrics<R>,
    ) {
    }

    /// Invoked when an actor is removed from the battle.
    ///
    /// The provided implementation does nothing.
    fn on_actor_removed(
        &self,
        _model: &mut Self::RoundsModel,
        _actor: &dyn Actor<R>,
        _entropy: &mut Entropy<R>,
        _metrics: &mut WriteMetrics<R>,
    ) {
    }
}

/// Type to represent a rounds seed.
/// It is used to bootstrap the `RoundsModel` for a game.
pub type RoundsSeed<R> = <<R as BattleRules>::RR as RoundsRules<R>>::RoundsSeed;

/// Type to store all information about the order of rounds in the game.
///
/// The round model should contain enough data to compute which actor will act next.
/// It might be based on a round-robin policy, on the actor's quickness or on any other
/// arbitrary metric.
pub type RoundsModel<R> = <<R as BattleRules>::RR as RoundsRules<R>>::RoundsModel;

/// Event to make an actor start a new round.
///
/// When an actor starts a round all his status effects will be updated.
///
/// # Examples
/// ```
/// use weasel::battle::{Battle, BattleRules};
/// use weasel::creature::CreateCreature;
/// use weasel::entity::EntityId;
/// use weasel::event::EventTrigger;
/// use weasel::round::StartRound;
/// use weasel::team::CreateTeam;
/// use weasel::{Server, battle_rules, rules::empty::*};
///
/// battle_rules! {}
///
/// let battle = Battle::builder(CustomRules::new()).build();
/// let mut server = Server::builder(battle).build();
///
/// let team_id = 1;
/// CreateTeam::trigger(&mut server, team_id).fire().unwrap();
/// let creature_id = 1;
/// let position = ();
/// CreateCreature::trigger(&mut server, creature_id, team_id, position)
///     .fire()
///     .unwrap();
///
/// StartRound::trigger(&mut server, EntityId::Creature(creature_id))
///     .fire()
///     .unwrap();
/// assert!(server
///     .battle()
///     .rounds()
///     .state()
///     .has_actor(&EntityId::Creature(creature_id)));
/// ```
#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct StartRound<R: BattleRules> {
    #[cfg_attr(
        feature = "serialization",
        serde(bound(
            serialize = "Vec<EntityId<R>>: Serialize",
            deserialize = "Vec<EntityId<R>>: Deserialize<'de>"
        ))
    )]
    ids: Vec<EntityId<R>>,
}

impl<R: BattleRules> StartRound<R> {
    /// Returns a trigger for this event, to start a round with a single actor.
    pub fn trigger<P: EventProcessor<R>>(
        processor: &mut P,
        id: EntityId<R>,
    ) -> StartRoundTrigger<R, P> {
        StartRoundTrigger {
            processor,
            ids: vec![id],
        }
    }

    /// Returns a trigger for this event, to start a round with a list of actors.\
    /// Duplicated ids will be dropped during the event's processing.
    pub fn trigger_with_actors<P, I>(processor: &mut P, ids: I) -> StartRoundTrigger<R, P>
    where
        P: EventProcessor<R>,
        I: IntoIterator<Item = EntityId<R>>,
    {
        StartRoundTrigger {
            processor,
            ids: ids.into_iter().collect(),
        }
    }

    /// Returns the ids of the entities that will start the round.
    pub fn ids(&self) -> &Vec<EntityId<R>> {
        &self.ids
    }
}

impl<R: BattleRules> Debug for StartRound<R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "StartRound {{ ids: {:?} }}", self.ids)
    }
}

impl<R: BattleRules> Clone for StartRound<R> {
    fn clone(&self) -> Self {
        StartRound {
            ids: self.ids.clone(),
        }
    }
}

impl<R: BattleRules + 'static> Event<R> for StartRound<R> {
    fn verify(&self, battle: &Battle<R>) -> WeaselResult<(), R> {
        // Verify if a round can start.
        if let RoundState::Started(_) = battle.rounds().state() {
            return Err(WeaselError::RoundInProgress);
        }
        for id in &self.ids {
            // Verify if entity is an actor.
            if !id.is_actor() {
                return Err(WeaselError::NotAnActor(id.clone()));
            }
            // Verify if entity exists.
            if let Some(actor) = battle.entities().actor(&id) {
                // Verify if actor is eligible.
                if !battle.rounds().eligible(actor) {
                    return Err(WeaselError::ActorNotEligible(id.clone()));
                }
            } else {
                return Err(WeaselError::EntityNotFound(id.clone()));
            }
        }
        Ok(())
    }

    fn apply(&self, battle: &mut Battle<R>, event_queue: &mut Option<EventQueue<R>>) {
        // Set the round state.
        let actors_ids: IndexSet<_> = self.ids.iter().cloned().collect();
        battle
            .state
            .rounds
            .set_state(RoundState::Started(actors_ids.clone()));
        battle
            .metrics
            .write_handle()
            .add_system_u64(ROUNDS_STARTED, 1)
            .unwrap_or_else(|err| panic!("constraint violated: {:?}", err));
        // Perform some operations on every actor.
        for id in &actors_ids {
            let metrics = &mut battle.metrics.write_handle();
            // Get the actor.
            let actor = battle
                .state
                .entities
                .actor(id)
                .unwrap_or_else(|| panic!("constraint violated: actor {:?} not found", id));
            // Invoke `RoundRules` callback.
            battle.state.rounds.rules.on_start(
                &battle.state.entities,
                &battle.state.space,
                &mut battle.state.rounds.model,
                actor,
                &mut battle.entropy,
                metrics,
            );
            // Invoke `CharacterRules` callback.
            battle.rules.actor_rules().on_round_start(
                &battle.state,
                actor,
                event_queue,
                &mut battle.entropy,
                metrics,
            );
            // Update all statuses afflicting the actor.
            update_statuses(id, battle, event_queue)
                .unwrap_or_else(|err| panic!("constraint violated: {:?}", err));
        }
    }

    fn kind(&self) -> EventKind {
        EventKind::StartRound
    }

    fn box_clone(&self) -> Box<dyn Event<R> + Send> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn rights<'a>(&'a self, battle: &'a Battle<R>) -> EventRights<'a, R> {
        // Collect all teams involved out of the list of actors.
        let mut teams = Vec::new();
        for id in &self.ids {
            let actor =
                battle.state.entities.actor(id).unwrap_or_else(|| {
                    panic!("constraint violated: actor {:?} not found", id.clone())
                });
            teams.push(actor.team_id());
        }
        EventRights::Teams(teams)
    }
}

/// Trigger to build and fire a `StartRound` event.
pub struct StartRoundTrigger<'a, R, P>
where
    R: BattleRules,
    P: EventProcessor<R>,
{
    processor: &'a mut P,
    ids: Vec<EntityId<R>>,
}

impl<'a, R, P> EventTrigger<'a, R, P> for StartRoundTrigger<'a, R, P>
where
    R: BattleRules + 'static,
    P: EventProcessor<R>,
{
    fn processor(&'a mut self) -> &'a mut P {
        self.processor
    }

    /// Returns an `StartRound` event.
    fn event(&self) -> Box<dyn Event<R> + Send> {
        Box::new(StartRound {
            ids: self.ids.clone(),
        })
    }
}

/// Event to end the current round.
///
/// # Examples
/// ```
/// use weasel::battle::{Battle, BattleRules};
/// use weasel::creature::CreateCreature;
/// use weasel::entity::EntityId;
/// use weasel::event::EventTrigger;
/// use weasel::round::{EndRound, RoundState, StartRound};
/// use weasel::team::CreateTeam;
/// use weasel::{Server, battle_rules, rules::empty::*};
///
/// battle_rules! {}
///
/// let battle = Battle::builder(CustomRules::new()).build();
/// let mut server = Server::builder(battle).build();
///
/// let team_id = 1;
/// CreateTeam::trigger(&mut server, team_id).fire().unwrap();
/// let creature_id = 1;
/// let position = ();
/// CreateCreature::trigger(&mut server, creature_id, team_id, position)
///     .fire()
///     .unwrap();
/// StartRound::trigger(&mut server, EntityId::Creature(creature_id))
///     .fire()
///     .unwrap();
///
/// EndRound::trigger(&mut server).fire().unwrap();
/// assert_eq!(*server.battle().rounds().state(), RoundState::Ready);
/// ```
#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct EndRound<R> {
    #[cfg_attr(feature = "serialization", serde(skip))]
    _phantom: PhantomData<R>,
}

impl<R: BattleRules> EndRound<R> {
    /// Returns a trigger for this event.
    pub fn trigger<P: EventProcessor<R>>(processor: &mut P) -> EndRoundTrigger<R, P> {
        EndRoundTrigger {
            processor,
            _phantom: PhantomData,
        }
    }
}

impl<R> Debug for EndRound<R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "EndRound {{ }}")
    }
}

impl<R> Clone for EndRound<R> {
    fn clone(&self) -> Self {
        EndRound {
            _phantom: PhantomData,
        }
    }
}

impl<R: BattleRules + 'static> Event<R> for EndRound<R> {
    fn verify(&self, battle: &Battle<R>) -> WeaselResult<(), R> {
        // Verify if the round can end.
        if let RoundState::Ready = battle.rounds().state() {
            return Err(WeaselError::NoRoundInProgress);
        }
        Ok(())
    }

    fn apply(&self, battle: &mut Battle<R>, event_queue: &mut Option<EventQueue<R>>) {
        let actors_ids = if let RoundState::Started(actors) = battle.state.rounds.state() {
            actors.clone()
        } else {
            panic!("constraint violated: end round called when state is not started");
        };
        // End the round for each actor.
        for actor_id in actors_ids {
            let actor = battle.state.entities.actor(&actor_id).unwrap_or_else(|| {
                panic!(
                    "constraint violated: actor {:?} not found",
                    actor_id.clone()
                )
            });
            let metrics = &mut battle.metrics.write_handle();
            // Invoke `CharacterRules` callback.
            battle.rules.actor_rules().on_round_end(
                &battle.state,
                actor,
                event_queue,
                &mut battle.entropy,
                metrics,
            );
            // Invoke `RoundRules` callback.
            battle.state.rounds.on_end(
                &battle.state.entities,
                &battle.state.space,
                actor,
                &mut battle.entropy,
                metrics,
            );
            // Check teams' objectives.
            Battle::check_objectives(
                &battle.state,
                &battle.rules.team_rules(),
                &battle.metrics.read_handle(),
                event_queue,
                Checkpoint::RoundEnd,
            );
        }
        // Set the round state.
        battle.state.rounds.set_state(RoundState::Ready);
    }

    fn kind(&self) -> EventKind {
        EventKind::EndRound
    }

    fn box_clone(&self) -> Box<dyn Event<R> + Send> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn rights<'a>(&'a self, battle: &'a Battle<R>) -> EventRights<'a, R> {
        let actors = if let RoundState::Started(actors) = battle.state.rounds.state() {
            actors
        } else {
            panic!("constraint violated: end round called when state is not started");
        };
        // Collect the rights to all teams involved.
        let mut teams = Vec::new();
        for actor_id in actors {
            let actor = battle.state.entities.actor(actor_id).unwrap_or_else(|| {
                panic!(
                    "constraint violated: actor {:?} not found",
                    actor_id.clone()
                )
            });
            teams.push(actor.team_id());
        }
        EventRights::Teams(teams)
    }
}

/// Trigger to build and fire an `EndRound` event.
pub struct EndRoundTrigger<'a, R, P>
where
    R: BattleRules,
    P: EventProcessor<R>,
{
    processor: &'a mut P,
    _phantom: PhantomData<R>,
}

impl<'a, R, P> EventTrigger<'a, R, P> for EndRoundTrigger<'a, R, P>
where
    R: BattleRules + 'static,
    P: EventProcessor<R>,
{
    fn processor(&'a mut self) -> &'a mut P {
        self.processor
    }

    /// Returns an `EndRound` event.
    fn event(&self) -> Box<dyn Event<R> + Send> {
        Box::new(EndRound {
            _phantom: self._phantom,
        })
    }
}

/// Event to reset the rounds model.
///
/// This event can be fired only if no round is in progress.
///
/// # Examples
/// ```
/// use weasel::battle::{Battle, BattleRules};
/// use weasel::event::{EventTrigger, EventKind};
/// use weasel::round::ResetRounds;
/// use weasel::{Server, battle_rules, rules::empty::*};
///
/// battle_rules! {}
///
/// let battle = Battle::builder(CustomRules::new()).build();
/// let mut server = Server::builder(battle).build();
///
/// ResetRounds::trigger(&mut server).fire().unwrap();
/// assert_eq!(
///     server.battle().history().events()[0].kind(),
///     EventKind::ResetRounds
/// );
/// ```
#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct ResetRounds<R: BattleRules> {
    #[cfg_attr(
        feature = "serialization",
        serde(bound(
            serialize = "Option<RoundsSeed<R>>: Serialize",
            deserialize = "Option<RoundsSeed<R>>: Deserialize<'de>"
        ))
    )]
    seed: Option<RoundsSeed<R>>,
}

impl<R: BattleRules> ResetRounds<R> {
    /// Returns a trigger for this event.
    pub fn trigger<P: EventProcessor<R>>(processor: &mut P) -> ResetRoundsTrigger<R, P> {
        ResetRoundsTrigger {
            processor,
            seed: None,
        }
    }

    /// Returns the new seed.
    pub fn seed(&self) -> &Option<RoundsSeed<R>> {
        &self.seed
    }
}

impl<R: BattleRules> Debug for ResetRounds<R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "ResetRounds {{ seed: {:?} }}", self.seed)
    }
}

impl<R: BattleRules> Clone for ResetRounds<R> {
    fn clone(&self) -> Self {
        ResetRounds {
            seed: self.seed.clone(),
        }
    }
}

impl<R: BattleRules + 'static> Event<R> for ResetRounds<R> {
    fn verify(&self, battle: &Battle<R>) -> WeaselResult<(), R> {
        // Verify that no round is in progress.
        if let RoundState::Started(_) = battle.rounds().state() {
            return Err(WeaselError::RoundInProgress);
        }
        Ok(())
    }

    fn apply(&self, battle: &mut Battle<R>, _: &mut Option<EventQueue<R>>) {
        battle.state.rounds.regenerate_model(&self.seed);
    }

    fn kind(&self) -> EventKind {
        EventKind::ResetRounds
    }

    fn box_clone(&self) -> Box<dyn Event<R> + Send> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Trigger to build and fire a `ResetRounds` event.
pub struct ResetRoundsTrigger<'a, R, P>
where
    R: BattleRules,
    P: EventProcessor<R>,
{
    processor: &'a mut P,
    seed: Option<RoundsSeed<R>>,
}

impl<'a, R, P> ResetRoundsTrigger<'a, R, P>
where
    R: BattleRules + 'static,
    P: EventProcessor<R>,
{
    /// Adds a seed to drive the generation of the new rounds model.
    pub fn seed(&'a mut self, seed: RoundsSeed<R>) -> &'a mut ResetRoundsTrigger<'a, R, P> {
        self.seed = Some(seed);
        self
    }
}

impl<'a, R, P> EventTrigger<'a, R, P> for ResetRoundsTrigger<'a, R, P>
where
    R: BattleRules + 'static,
    P: EventProcessor<R>,
{
    fn processor(&'a mut self) -> &'a mut P {
        self.processor
    }

    /// Returns a `ResetRounds` event.
    fn event(&self) -> Box<dyn Event<R> + Send> {
        Box::new(ResetRounds {
            seed: self.seed.clone(),
        })
    }
}

/// Event to perform a collective round for the environment's inanimate entities.\
/// The purpose of this event is to update the statuses of all objects.
///
/// # Examples
/// ```
/// use weasel::battle::{Battle, BattleRules};
/// use weasel::event::EventTrigger;
/// use weasel::metric::system::ROUNDS_STARTED;
/// use weasel::round::{EnvironmentRound};
/// use weasel::{Server, battle_rules, rules::empty::*};
///
/// battle_rules! {}
///
/// let battle = Battle::builder(CustomRules::new()).build();
/// let mut server = Server::builder(battle).build();
///
/// EnvironmentRound::trigger(&mut server).fire().unwrap();
/// assert_eq!(
///     server.battle().metrics().system_u64(ROUNDS_STARTED),
///     Some(1)
/// );
/// ```
#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct EnvironmentRound<R> {
    #[cfg_attr(feature = "serialization", serde(skip))]
    _phantom: PhantomData<R>,
}

impl<R: BattleRules> EnvironmentRound<R> {
    /// Returns a trigger for this event.
    pub fn trigger<P: EventProcessor<R>>(processor: &mut P) -> EnvironmentRoundTrigger<R, P> {
        EnvironmentRoundTrigger {
            processor,
            _phantom: PhantomData,
        }
    }
}

impl<R> Debug for EnvironmentRound<R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "EnvironmentRound {{ }}")
    }
}

impl<R> Clone for EnvironmentRound<R> {
    fn clone(&self) -> Self {
        EnvironmentRound {
            _phantom: PhantomData,
        }
    }
}

impl<R: BattleRules + 'static> Event<R> for EnvironmentRound<R> {
    fn verify(&self, battle: &Battle<R>) -> WeaselResult<(), R> {
        // Verify that no other round is in progress.
        if let RoundState::Started(_) = battle.rounds().state() {
            return Err(WeaselError::RoundInProgress);
        }
        Ok(())
    }

    fn apply(&self, battle: &mut Battle<R>, event_queue: &mut Option<EventQueue<R>>) {
        // Increase metrics.
        battle
            .metrics_mut()
            .add_system_u64(ROUNDS_STARTED, 1)
            .unwrap_or_else(|err| panic!("constraint violated: {:?}", err));
        // Update the statuses of all objects.
        let objects_ids: Vec<_> = battle
            .entities()
            .objects()
            .map(|object| object.entity_id())
            .cloned()
            .collect();
        for object_id in objects_ids {
            update_statuses(&object_id, battle, event_queue)
                .unwrap_or_else(|err| panic!("constraint violated: {:?}", err));
        }
    }

    fn kind(&self) -> EventKind {
        EventKind::EnvironmentRound
    }

    fn box_clone(&self) -> Box<dyn Event<R> + Send> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Trigger to build and fire an `EnvironmentRound` event.
pub struct EnvironmentRoundTrigger<'a, R, P>
where
    R: BattleRules,
    P: EventProcessor<R>,
{
    processor: &'a mut P,
    _phantom: PhantomData<R>,
}

impl<'a, R, P> EventTrigger<'a, R, P> for EnvironmentRoundTrigger<'a, R, P>
where
    R: BattleRules + 'static,
    P: EventProcessor<R>,
{
    fn processor(&'a mut self) -> &'a mut P {
        self.processor
    }

    /// Returns an `EnvironmentRound` event.
    fn event(&self) -> Box<dyn Event<R> + Send> {
        Box::new(EnvironmentRound {
            _phantom: self._phantom,
        })
    }
}
