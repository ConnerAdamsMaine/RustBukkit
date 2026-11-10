use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use parking_lot::Mutex;
use rustbukkit_chunks::{BlockKind, BlockPos, World};
use rustbukkit_inventory::{Inventory, ItemKind, ItemStack};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::player_location::PlayerLocation;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LivingState {
    pub health:                f32,
    pub max_health:            f32,
    pub invulnerability_ticks: u8,
}

impl Default for LivingState {
    fn default() -> Self {
        Self {
            health:                20.0,
            max_health:            20.0,
            invulnerability_ticks: 0,
        }
    }
}

impl LivingState {
    pub fn alive(self) -> bool {
        self.health > 0.0
    }

    fn apply_damage(&mut self, damage: Damage) -> bool {
        if !self.alive()
            || self.invulnerability_ticks > 0
            || !damage.amount.is_finite()
            || damage.amount <= 0.0
        {
            return false;
        }
        self.health = (self.health - damage.amount).max(0.0);
        self.invulnerability_ticks = 10;
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageKind {
    Melee,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageSource {
    Player(Uuid),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Damage {
    pub source: DamageSource,
    pub kind:   DamageKind,
    pub amount: f32,
}

#[derive(Debug, Clone)]
pub struct PlayerSnapshot {
    pub uuid:         Uuid,
    pub entity_id:    i32,
    pub username:     String,
    pub location:     PlayerLocation,
    pub living:       LivingState,
    pub velocity:     Vec3,
    last_attack_tick: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum PresenceEvent {
    Joined(PlayerSnapshot),
    Moved(PlayerSnapshot),
    Left { uuid: Uuid, entity_id: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityId(i32);

impl EntityId {
    pub fn raw(self) -> i32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub fn new(x: f64, y: f64, z: f64) -> Option<Self> {
        (x.is_finite()
            && y.is_finite()
            && z.is_finite()
            && x.abs() <= 29_999_984.0
            && z.abs() <= 29_999_984.0
            && (-64.0..=320.0).contains(&y))
        .then_some(Self { x, y, z })
    }

    fn chunk(self) -> (i32, i32) {
        ((self.x / 16.0).floor() as i32, (self.z / 16.0).floor() as i32)
    }
}

#[derive(Debug, Clone)]
pub struct ItemEntity {
    pub id:                 EntityId,
    pub uuid:               Uuid,
    pub position:           Vec3,
    pub velocity:           Vec3,
    pub stack:              ItemStack,
    pub age_ticks:          u32,
    pub pickup_delay_ticks: u16,
}

#[derive(Debug, Clone)]
pub enum EntitySnapshot {
    Player(PlayerSnapshot),
    Item(ItemEntity),
}

#[derive(Debug, Clone)]
pub enum EntityEvent {
    Spawn(EntitySnapshot),
    Update(EntitySnapshot),
    Remove { id: EntityId },
}

#[derive(Default)]
struct EntityState {
    players:           HashMap<Uuid, PlayerSnapshot>,
    pending_players:   HashSet<Uuid>,
    player_ids:        HashMap<EntityId, Uuid>,
    player_entity_ids: HashMap<Uuid, EntityId>,
    items:             HashMap<EntityId, ItemEntity>,
    chunks:            HashMap<(i32, i32), HashSet<EntityId>>,
}

impl EntityState {
    fn index(&mut self, id: EntityId, chunk: (i32, i32)) {
        self.chunks.entry(chunk).or_default().insert(id);
    }

    fn unindex(&mut self, id: EntityId, chunk: (i32, i32)) {
        if let Some(ids) = self.chunks.get_mut(&chunk) {
            ids.remove(&id);
            if ids.is_empty() {
                self.chunks.remove(&chunk);
            }
        }
    }

    fn move_index(&mut self, id: EntityId, old: (i32, i32), new: (i32, i32)) {
        if old != new {
            self.unindex(id, old);
            self.index(id, new);
        }
    }
}

pub struct EntityManager {
    state:          Mutex<EntityState>,
    next_entity_id: AtomicI32,
    tick:           AtomicU64,
    events:         broadcast::Sender<PresenceEvent>,
    entity_events:  broadcast::Sender<EntityEvent>,
}

pub type SessionRegistry = EntityManager;

impl Default for EntityManager {
    fn default() -> Self {
        let (events, _) = broadcast::channel(1024);
        let (entity_events, _) = broadcast::channel(1024);
        Self {
            state: Mutex::new(EntityState::default()),
            next_entity_id: AtomicI32::new(2),
            tick: AtomicU64::new(0),
            events,
            entity_events,
        }
    }
}

impl EntityManager {
    fn allocate_id(&self) -> Option<EntityId> {
        self.next_entity_id
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .ok()
            .map(EntityId)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PresenceEvent> {
        self.events.subscribe()
    }

    pub fn subscribe_entities(&self) -> broadcast::Receiver<EntityEvent> {
        self.entity_events.subscribe()
    }

    pub fn player_count(&self) -> usize {
        self.state.lock().players.len()
    }

    pub fn reserve(self: &Arc<Self>, player: Uuid) -> Option<SessionReservation> {
        let mut state = self.state.lock();
        if state.players.contains_key(&player) || !state.pending_players.insert(player) {
            return None;
        }
        Some(SessionReservation {
            registry: Arc::clone(self),
            player,
            pending: true,
        })
    }

    pub fn snapshot_except(&self, player: Uuid) -> Vec<PlayerSnapshot> {
        self.state
            .lock()
            .players
            .values()
            .filter(|entry| entry.uuid != player)
            .cloned()
            .collect()
    }

    #[cfg(test)]
    pub fn player(&self, player: Uuid) -> Option<PlayerSnapshot> {
        self.state.lock().players.get(&player).cloned()
    }

    pub fn item(&self, id: EntityId) -> Option<ItemEntity> {
        self.state.lock().items.get(&id).cloned()
    }

    pub fn snapshot_items_near(&self, position: Vec3, radius: f64) -> Vec<ItemEntity> {
        let min_x = ((position.x - radius) / 16.0).floor() as i32;
        let max_x = ((position.x + radius) / 16.0).floor() as i32;
        let min_z = ((position.z - radius) / 16.0).floor() as i32;
        let max_z = ((position.z + radius) / 16.0).floor() as i32;
        let state = self.state.lock();
        let mut nearby = Vec::new();
        for x in min_x..=max_x {
            for z in min_z..=max_z {
                if let Some(ids) = state.chunks.get(&(x, z)) {
                    nearby.extend(
                        ids.iter()
                            .filter_map(|id| state.items.get(id))
                            .filter(|item| {
                                let dx = item.position.x - position.x;
                                let dy = item.position.y - position.y;
                                let dz = item.position.z - position.z;
                                dx * dx + dy * dy + dz * dz <= radius * radius
                            })
                            .cloned(),
                    );
                }
            }
        }
        nearby
    }

    pub fn spawn_item(
        &self,
        stack: ItemStack,
        position: Vec3,
        velocity: Vec3,
        pickup_delay_ticks: u16,
    ) -> Option<EntityId> {
        let position = Vec3::new(position.x, position.y, position.z)?;
        if !(velocity.x.is_finite() && velocity.y.is_finite() && velocity.z.is_finite()) {
            return None;
        }
        let id = self.allocate_id()?;
        let item = ItemEntity {
            id,
            uuid: Uuid::new_v4(),
            position,
            velocity,
            stack,
            age_ticks: 0,
            pickup_delay_ticks,
        };
        {
            let mut state = self.state.lock();
            state.index(id, position.chunk());
            state.items.insert(id, item.clone());
        }
        let _ = self
            .entity_events
            .send(EntityEvent::Spawn(EntitySnapshot::Item(item)));
        Some(id)
    }

    pub fn drop_from_player(&self, stack: ItemStack, location: PlayerLocation) -> Option<EntityId> {
        let yaw = f64::from(location.yaw).to_radians();
        let direction = Vec3 {
            x: -yaw.sin(),
            y: 0.0,
            z: yaw.cos(),
        };
        self.spawn_item(
            stack,
            Vec3 {
                x: location.x + direction.x * 0.3,
                y: location.y + 1.0,
                z: location.z + direction.z * 0.3,
            },
            Vec3 {
                x: direction.x * 0.1,
                y: 0.2,
                z: direction.z * 0.1,
            },
            10,
        )
    }

    pub fn drop_from_block(&self, block: BlockKind, position: BlockPos) -> Option<EntityId> {
        let kind = match block {
            BlockKind::Air | BlockKind::Bedrock | BlockKind::Water => return None,
            BlockKind::Dirt | BlockKind::GrassBlock => ItemKind::Dirt,
            BlockKind::Stone | BlockKind::Cobblestone => ItemKind::Cobblestone,
            BlockKind::Sand => ItemKind::Sand,
        };
        self.spawn_item(
            ItemStack::new(kind, 1)?,
            Vec3 {
                x: f64::from(position.x) + 0.5,
                y: f64::from(position.y) + 0.5,
                z: f64::from(position.z) + 0.5,
            },
            Vec3 {
                x: 0.0,
                y: 0.1,
                z: 0.0,
            },
            10,
        )
    }

    #[cfg(test)]
    fn remove_item(&self, id: EntityId) -> bool {
        {
            let mut state = self.state.lock();
            let Some(item) = state.items.remove(&id) else {
                return false;
            };
            state.unindex(id, item.position.chunk());
        }
        let _ = self.entity_events.send(EntityEvent::Remove { id });
        true
    }

    pub fn tick(&self, world: &World) {
        const DESPAWN_TICKS: u32 = 6_000;
        self.tick.fetch_add(1, Ordering::Relaxed);
        let mut updates = Vec::new();
        let mut movements = Vec::new();
        let mut removals = Vec::new();
        {
            let mut state = self.state.lock();
            for player in state.players.values_mut() {
                player.living.invulnerability_ticks = player.living.invulnerability_ticks.saturating_sub(1);
            }
            let ids: Vec<_> = state.items.keys().copied().collect();
            for id in ids {
                let Some(item) = state.items.get_mut(&id) else {
                    continue;
                };
                item.age_ticks = item.age_ticks.saturating_add(1);
                item.pickup_delay_ticks = item.pickup_delay_ticks.saturating_sub(1);
                if item.age_ticks >= DESPAWN_TICKS || item.position.y < -64.0 {
                    removals.push((id, item.position.chunk()));
                    continue;
                }
                let old_position = item.position;
                let old_velocity = item.velocity;
                item.velocity.y = (item.velocity.y - 0.04).max(-3.92);
                let mut next = Vec3 {
                    x: item.position.x + item.velocity.x,
                    y: item.position.y + item.velocity.y,
                    z: item.position.z + item.velocity.z,
                };
                if item.velocity.y < 0.0 {
                    let first_y = next.y.floor() as i32 - 1;
                    let last_y = old_position.y.floor() as i32;
                    for y in first_y..=last_y {
                        if let Some(block) = BlockPos::new(next.x.floor() as i32, y, next.z.floor() as i32)
                            && world.get_block(block).is_solid()
                            && old_position.y >= f64::from(y + 1)
                            && next.y <= f64::from(y + 1)
                        {
                            next.y = f64::from(y + 1);
                            item.velocity.y = 0.0;
                            item.velocity.x *= 0.6;
                            item.velocity.z *= 0.6;
                            break;
                        }
                    }
                }
                if item.velocity.y != 0.0 {
                    item.velocity.x *= 0.98;
                    item.velocity.y *= 0.98;
                    item.velocity.z *= 0.98;
                }
                item.position = next;
                if old_position != item.position || old_velocity != item.velocity {
                    movements.push((id, old_position.chunk(), item.position.chunk()));
                    updates.push(item.clone());
                }
            }
            for (id, old_chunk, new_chunk) in movements {
                state.move_index(id, old_chunk, new_chunk);
            }
            for (id, chunk) in &removals {
                state.items.remove(id);
                state.unindex(*id, *chunk);
            }
        }
        for item in updates {
            let _ = self
                .entity_events
                .send(EntityEvent::Update(EntitySnapshot::Item(item)));
        }
        for (id, _) in removals {
            let _ = self.entity_events.send(EntityEvent::Remove { id });
        }
        self.apply_player_physics(world);
    }

    pub fn pickup_near(&self, player: Uuid, inventory: &mut Inventory) -> bool {
        const PICKUP_RADIUS: f64 = 1.5;
        let mut updates = Vec::new();
        let mut removals = Vec::new();
        {
            let mut state = self.state.lock();
            let Some(location) = state.players.get(&player).map(|p| p.location) else {
                return false;
            };
            let center = Vec3 {
                x: location.x,
                y: location.y + 0.5,
                z: location.z,
            };
            let min_x = ((center.x - PICKUP_RADIUS) / 16.0).floor() as i32;
            let max_x = ((center.x + PICKUP_RADIUS) / 16.0).floor() as i32;
            let min_z = ((center.z - PICKUP_RADIUS) / 16.0).floor() as i32;
            let max_z = ((center.z + PICKUP_RADIUS) / 16.0).floor() as i32;
            let mut candidates = Vec::new();
            for x in min_x..=max_x {
                for z in min_z..=max_z {
                    if let Some(ids) = state.chunks.get(&(x, z)) {
                        candidates.extend(ids.iter().copied().filter(|id| state.items.contains_key(id)));
                    }
                }
            }
            for id in candidates {
                let Some(item) = state.items.get_mut(&id) else {
                    continue;
                };
                if item.pickup_delay_ticks > 0 {
                    continue;
                }
                let dx = item.position.x - center.x;
                let dy = item.position.y - center.y;
                let dz = item.position.z - center.z;
                if dx * dx + dy * dy + dz * dz > PICKUP_RADIUS * PICKUP_RADIUS {
                    continue;
                }
                let previous = item.stack;
                match inventory.insert(previous) {
                    Some(remainder) if remainder == previous => {}
                    Some(remainder) => {
                        item.stack = remainder;
                        updates.push(item.clone());
                    }
                    None => removals.push((id, item.position.chunk())),
                }
            }
            for (id, chunk) in &removals {
                state.items.remove(id);
                state.unindex(*id, *chunk);
            }
        }
        let changed = !updates.is_empty() || !removals.is_empty();
        for item in updates {
            let _ = self
                .entity_events
                .send(EntityEvent::Update(EntitySnapshot::Item(item)));
        }
        for (id, _) in removals {
            let _ = self.entity_events.send(EntityEvent::Remove { id });
        }
        changed
    }

    pub fn attack_player(&self, attacker: Uuid, target_entity_id: i32) -> Option<PlayerSnapshot> {
        const MELEE_REACH_SQUARED: f64 = 9.0;
        const ATTACK_COOLDOWN_TICKS: u64 = 10;
        let tick = self.tick.load(Ordering::Relaxed);
        let mut state = self.state.lock();
        let attacker_id = *state.player_entity_ids.get(&attacker)?;
        if attacker_id.raw() == target_entity_id {
            return None;
        }
        let target_uuid = *state.player_ids.get(&EntityId(target_entity_id))?;
        let source = state.players.get(&attacker)?;
        if !source.living.alive()
            || source
                .last_attack_tick
                .is_some_and(|last| tick.saturating_sub(last) < ATTACK_COOLDOWN_TICKS)
        {
            return None;
        }
        let target = state.players.get(&target_uuid)?;
        if !target.living.alive() || target.living.invulnerability_ticks > 0 {
            return None;
        }
        let dx = target.location.x - source.location.x;
        let dy = target.location.y - source.location.y;
        let dz = target.location.z - source.location.z;
        if dx * dx + dy * dy + dz * dz > MELEE_REACH_SQUARED {
            return None;
        }
        let length = (dx * dx + dz * dz).sqrt();
        let (kx, kz) = if length > f64::EPSILON {
            (dx / length * 0.4, dz / length * 0.4)
        } else {
            (0.0, 0.0)
        };
        let target = state.players.get_mut(&target_uuid)?;
        if !target.living.apply_damage(Damage {
            source: DamageSource::Player(attacker),
            kind:   DamageKind::Melee,
            amount: 1.0,
        }) {
            return None;
        }
        target.velocity = Vec3 { x: kx, y: 0.4, z: kz };
        let snapshot = target.clone();
        state.players.get_mut(&attacker)?.last_attack_tick = Some(tick);
        drop(state);
        let _ = self
            .entity_events
            .send(EntityEvent::Update(EntitySnapshot::Player(snapshot.clone())));
        let _ = self.events.send(PresenceEvent::Moved(snapshot.clone()));
        Some(snapshot)
    }

    pub fn apply_player_physics(&self, world: &World) {
        let mut updates = Vec::new();
        {
            let mut state = self.state.lock();
            for (&uuid, player) in &mut state.players {
                if player.velocity.x == 0.0 && player.velocity.y == 0.0 && player.velocity.z == 0.0 {
                    continue;
                }
                let old_chunk = player.location.chunk();
                let old_y = player.location.y;
                player.location.x += player.velocity.x;
                player.location.y += player.velocity.y;
                player.location.z += player.velocity.z;
                if player.velocity.y < 0.0 {
                    let first_y = player.location.y.floor() as i32 - 1;
                    let last_y = old_y.floor() as i32;
                    for y in first_y..=last_y {
                        if let Some(block) = BlockPos::new(
                            player.location.x.floor() as i32,
                            y,
                            player.location.z.floor() as i32,
                        ) && world.get_block(block).is_solid()
                            && old_y >= f64::from(y + 1)
                            && player.location.y <= f64::from(y + 1)
                        {
                            player.location.y = f64::from(y + 1);
                            player.velocity.y = 0.0;
                            break;
                        }
                    }
                }
                player.velocity.x *= 0.6;
                if player.velocity.y != 0.0 {
                    player.velocity.y = (player.velocity.y - 0.08) * 0.98;
                }
                player.velocity.z *= 0.6;
                if player.velocity.x.abs() < 0.01 {
                    player.velocity.x = 0.0;
                }
                if player.velocity.y.abs() < 0.01 {
                    player.velocity.y = 0.0;
                }
                if player.velocity.z.abs() < 0.01 {
                    player.velocity.z = 0.0;
                }
                updates.push((uuid, old_chunk, player.location.chunk(), player.clone()));
            }
            for (uuid, old, new, _) in &updates {
                if let Some(id) = state.player_entity_ids.get(uuid).copied() {
                    state.move_index(id, *old, *new);
                }
            }
        }
        for (_, _, _, player) in updates {
            let _ = self.events.send(PresenceEvent::Moved(player));
        }
    }

    pub fn respawn_player(&self, player: Uuid, spawn: PlayerLocation) -> Option<PlayerSnapshot> {
        let snapshot = {
            let mut state = self.state.lock();
            let id = *state.player_entity_ids.get(&player)?;
            let entity = state.players.get_mut(&player)?;
            if entity.living.alive() {
                return None;
            }
            let old_chunk = entity.location.chunk();
            entity.location = spawn;
            state.move_index(id, old_chunk, spawn.chunk());
            let entity = state.players.get_mut(&player)?;
            entity.living = LivingState::default();
            entity.velocity = Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            };
            entity.last_attack_tick = None;
            entity.clone()
        };
        let _ = self
            .entity_events
            .send(EntityEvent::Update(EntitySnapshot::Player(snapshot.clone())));
        let _ = self.events.send(PresenceEvent::Moved(snapshot.clone()));
        Some(snapshot)
    }

    pub fn is_block_occupied(&self, block: BlockPos) -> bool {
        let (x, y, z) = (f64::from(block.x), f64::from(block.y), f64::from(block.z));
        self.state.lock().players.values().any(|player| {
            let p = player.location;
            player.living.alive()
                && p.x + 0.3 > x
                && p.x - 0.3 < x + 1.0
                && p.y + 1.8 > y
                && p.y < y + 1.0
                && p.z + 0.3 > z
                && p.z - 0.3 < z + 1.0
        })
    }

    pub fn enter(
        self: &Arc<Self>,
        player: Uuid,
        username: String,
        location: PlayerLocation,
    ) -> Option<SessionRegistration> {
        self.reserve(player)?.promote(username, location)
    }
}

pub struct SessionReservation {
    registry: Arc<SessionRegistry>,
    player:   Uuid,
    pending:  bool,
}

impl SessionReservation {
    pub fn promote(mut self, username: String, location: PlayerLocation) -> Option<SessionRegistration> {
        let mut state = self.registry.state.lock();
        if !state.pending_players.contains(&self.player) || state.players.contains_key(&self.player) {
            drop(state);
            return None;
        }
        let Some(id) = self.registry.allocate_id() else {
            drop(state);
            return None;
        };
        let snapshot = PlayerSnapshot {
            uuid: self.player,
            entity_id: id.raw(),
            username,
            location,
            living: LivingState::default(),
            velocity: Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            last_attack_tick: None,
        };
        state.player_ids.insert(id, self.player);
        state.player_entity_ids.insert(self.player, id);
        state.index(id, location.chunk());
        state.players.insert(self.player, snapshot.clone());
        state.pending_players.remove(&self.player);
        self.pending = false;
        drop(state);
        let _ = self.registry.events.send(PresenceEvent::Joined(snapshot.clone()));
        let _ = self
            .registry
            .entity_events
            .send(EntityEvent::Spawn(EntitySnapshot::Player(snapshot)));
        Some(SessionRegistration {
            registry:  Arc::clone(&self.registry),
            player:    self.player,
            entity_id: id,
        })
    }
}

impl Drop for SessionReservation {
    fn drop(&mut self) {
        if self.pending {
            self.registry.state.lock().pending_players.remove(&self.player);
        }
    }
}

pub struct SessionRegistration {
    registry:  Arc<SessionRegistry>,
    player:    Uuid,
    entity_id: EntityId,
}

impl SessionRegistration {
    pub fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    pub fn update_location(&self, location: PlayerLocation) {
        let moved = {
            let mut state = self.registry.state.lock();
            let Some(entity) = state.players.get_mut(&self.player) else {
                return;
            };
            let old = entity.location.chunk();
            entity.location = location;
            state.move_index(self.entity_id, old, location.chunk());
            state.players.get(&self.player).cloned()
        };
        if let Some(player) = moved {
            let _ = self.registry.events.send(PresenceEvent::Moved(player));
        }
    }
}

impl Drop for SessionRegistration {
    fn drop(&mut self) {
        {
            let mut state = self.registry.state.lock();
            let Some(player) = state.players.remove(&self.player) else {
                return;
            };
            state.player_ids.remove(&self.entity_id);
            state.player_entity_ids.remove(&self.player);
            state.unindex(self.entity_id, player.location.chunk());
        }
        let _ = self.registry.events.send(PresenceEvent::Left {
            uuid:      self.player,
            entity_id: self.entity_id.raw(),
        });
        let _ = self
            .registry
            .entity_events
            .send(EntityEvent::Remove { id: self.entity_id });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use rustbukkit_chunks::{BlockPos, World};
    use rustbukkit_inventory::{Inventory, InventorySlot, ItemKind, ItemStack};
    use tokio::sync::broadcast::error::TryRecvError;
    use uuid::Uuid;

    use super::{EntityEvent, EntitySnapshot, PresenceEvent, SessionRegistry, Vec3};
    use crate::player_location::PlayerLocation;

    fn test_spawn() -> PlayerLocation {
        PlayerLocation::spawn(f64::from(World::new().spawn_y(0, 0)))
    }

    #[test]
    fn promotion_makes_a_reserved_player_visible_without_releasing_the_identity() {
        let registry = Arc::new(SessionRegistry::default());
        let mut presence = registry.subscribe();
        let mut entities = registry.subscribe_entities();
        let player = Uuid::new_v4();
        let reservation = registry.reserve(player).unwrap();

        assert_eq!(registry.player_count(), 0);
        assert!(registry.reserve(player).is_none());
        assert!(registry.enter(player, "A".into(), test_spawn()).is_none());
        assert!(matches!(presence.try_recv(), Err(TryRecvError::Empty)));
        assert!(matches!(entities.try_recv(), Err(TryRecvError::Empty)));

        let registration = reservation.promote("A".into(), test_spawn()).unwrap();
        assert_eq!(registry.player_count(), 1);
        assert!(registry.reserve(player).is_none());
        assert!(
            matches!(presence.try_recv().unwrap(), PresenceEvent::Joined(snapshot) if snapshot.uuid == player)
        );
        assert!(
            matches!(entities.try_recv().unwrap(), EntityEvent::Spawn(EntitySnapshot::Player(snapshot)) if snapshot.uuid == player)
        );

        drop(registration);
        assert_eq!(registry.player_count(), 0);
        assert!(matches!(presence.try_recv().unwrap(), PresenceEvent::Left { uuid, .. } if uuid == player));
        assert!(matches!(entities.try_recv().unwrap(), EntityEvent::Remove { .. }));
        assert!(registry.reserve(player).is_some());
    }

    #[test]
    fn failed_promotion_releases_the_reservation_without_publishing_a_player() {
        let registry = Arc::new(SessionRegistry::default());
        let mut presence = registry.subscribe();
        let mut entities = registry.subscribe_entities();
        let player = Uuid::new_v4();
        let reservation = registry.reserve(player).unwrap();
        registry.next_entity_id.store(i32::MAX, Ordering::Relaxed);

        assert!(reservation.promote("A".into(), test_spawn()).is_none());
        assert_eq!(registry.player_count(), 0);
        assert!(matches!(presence.try_recv(), Err(TryRecvError::Empty)));
        assert!(matches!(entities.try_recv(), Err(TryRecvError::Empty)));
        assert!(registry.reserve(player).is_some());
    }

    #[test]
    fn permits_reconnect_after_registration_drops() {
        let registry = Arc::new(SessionRegistry::default());
        let player = Uuid::new_v4();
        let active = registry.enter(player, "A".into(), test_spawn()).unwrap();
        assert!(registry.enter(player, "A".into(), test_spawn()).is_none());
        assert!(registry.enter(Uuid::new_v4(), "B".into(), test_spawn()).is_some());
        drop(active);
        assert!(registry.enter(player, "A".into(), test_spawn()).is_some());
    }

    #[test]
    fn broadcasts_position_and_removal() {
        let registry = Arc::new(SessionRegistry::default());
        let mut receiver = registry.subscribe();
        let player = Uuid::new_v4();
        let active = registry.enter(player, "A".into(), test_spawn()).unwrap();
        assert!(matches!(receiver.try_recv().unwrap(), PresenceEvent::Joined(_)));
        let mut moved = test_spawn();
        moved.x = 10.0;
        active.update_location(moved);
        assert!(
            matches!(receiver.try_recv().unwrap(), PresenceEvent::Moved(snapshot) if snapshot.location.x == 10.0)
        );
        drop(active);
        assert!(matches!(receiver.try_recv().unwrap(), PresenceEvent::Left { uuid, .. } if uuid == player));
    }

    #[test]
    fn player_body_blocks_placement_but_ground_beneath_does_not() {
        let registry = Arc::new(SessionRegistry::default());
        let spawn = test_spawn();
        let ground_y = spawn.y as i32 - 1;
        let body_y = spawn.y as i32;
        let registration = registry.enter(Uuid::new_v4(), "A".into(), spawn).unwrap();
        assert!(registry.is_block_occupied(BlockPos::new(0, body_y, 0).unwrap()));
        assert!(registry.is_block_occupied(BlockPos::new(0, body_y + 1, 0).unwrap()));
        assert!(!registry.is_block_occupied(BlockPos::new(0, ground_y, 0).unwrap()));
        assert!(!registry.is_block_occupied(BlockPos::new(2, body_y, 0).unwrap()));
        drop(registration);
        assert!(!registry.is_block_occupied(BlockPos::new(0, body_y, 0).unwrap()));
    }

    #[test]
    fn item_tick_and_local_spatial_queries_work_across_negative_chunks() {
        let manager = SessionRegistry::default();
        let world = World::new();
        let mut events = manager.subscribe_entities();
        let stack = ItemStack::new(ItemKind::Dirt, 4).unwrap();
        let id = manager
            .spawn_item(
                stack,
                Vec3 {
                    x: -0.5,
                    y: 65.0,
                    z: -0.5,
                },
                Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                2,
            )
            .unwrap();
        assert!(matches!(events.try_recv().unwrap(), EntityEvent::Spawn(EntitySnapshot::Item(_))));
        assert_eq!(
            manager
                .snapshot_items_near(
                    Vec3 {
                        x: -0.5,
                        y: 65.0,
                        z: -0.5,
                    },
                    1.0
                )
                .len(),
            1
        );
        manager.tick(&world);
        let item = manager.item(id).unwrap();
        assert_eq!(item.pickup_delay_ticks, 1);
        assert!(item.position.y < 65.0);
        assert!(matches!(events.try_recv().unwrap(), EntityEvent::Update(EntitySnapshot::Item(_))));
        assert!(manager.remove_item(id));
        assert!(matches!(events.try_recv().unwrap(), EntityEvent::Remove { .. }));
    }

    #[test]
    fn pickup_preserves_remainder_and_respects_delay() {
        let manager = Arc::new(SessionRegistry::default());
        let player = Uuid::new_v4();
        let mut spawn = test_spawn();
        spawn.y = 64.0;
        let _registration = manager.enter(player, "A".into(), spawn).unwrap();
        let stack = ItemStack::new(ItemKind::Dirt, 10).unwrap();
        let id = manager
            .spawn_item(
                stack,
                Vec3 {
                    x: 1.0,
                    y: 64.5,
                    z: 0.5,
                },
                Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                2,
            )
            .unwrap();
        let mut inventory = Inventory::empty(0).unwrap();
        for slot in 9..45 {
            inventory.set(InventorySlot::new(slot).unwrap(), ItemStack::new(ItemKind::Stone, 64));
        }
        inventory.set(InventorySlot::new(36).unwrap(), ItemStack::new(ItemKind::Dirt, 60));
        assert!(!manager.pickup_near(player, &mut inventory));
        manager.tick(&World::new());
        assert!(!manager.pickup_near(player, &mut inventory));
        manager.tick(&World::new());
        assert!(manager.pickup_near(player, &mut inventory));
        assert_eq!(inventory.get(InventorySlot::new(36).unwrap()).unwrap().count(), 64);
        assert_eq!(manager.item(id).unwrap().stack.count(), 6);
    }

    #[test]
    fn full_inventory_does_not_delete_drop() {
        let manager = Arc::new(SessionRegistry::default());
        let player = Uuid::new_v4();
        let mut spawn = test_spawn();
        spawn.y = 64.0;
        let _registration = manager.enter(player, "A".into(), spawn).unwrap();
        let stack = ItemStack::new(ItemKind::Dirt, 3).unwrap();
        let id = manager
            .spawn_item(
                stack,
                Vec3 {
                    x: 0.5,
                    y: 64.5,
                    z: 0.5,
                },
                Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                0,
            )
            .unwrap();
        let mut inventory = Inventory::empty(0).unwrap();
        for slot in 9..45 {
            inventory.set(InventorySlot::new(slot).unwrap(), ItemStack::new(ItemKind::Stone, 64));
        }
        inventory.set(InventorySlot::new(36).unwrap(), ItemStack::new(ItemKind::Stone, 64));
        assert!(!manager.pickup_near(player, &mut inventory));
        assert_eq!(manager.item(id).unwrap().stack, stack);
    }

    #[test]
    fn combat_checks_target_distance_cooldown_and_death_respawn() {
        let manager = Arc::new(SessionRegistry::default());
        let attacker = Uuid::new_v4();
        let victim = Uuid::new_v4();
        let attacker_registration = manager.enter(attacker, "A".into(), test_spawn()).unwrap();
        let mut location = test_spawn();
        location.x = 1.0;
        let _b = manager.enter(victim, "B".into(), location).unwrap();
        let target = manager.player(victim).unwrap().entity_id;
        assert!(manager.attack_player(attacker, target).is_some());
        assert_eq!(manager.player(victim).unwrap().living.health, 19.0);
        assert!(manager.attack_player(attacker, 999_999).is_none());
        for _ in 0..200 {
            if let Some(victim_state) = manager.player(victim) {
                let mut attack_position = victim_state.location;
                attack_position.y = victim_state.location.y;
                attacker_registration.update_location(attack_position);
                if victim_state.living.alive() {
                    let _ = manager.attack_player(attacker, target);
                }
            }
            manager.tick(&World::new());
        }
        assert_eq!(manager.player(victim).unwrap().living.health, 0.0);
        assert!(manager.respawn_player(victim, test_spawn()).is_some());
        assert_eq!(manager.player(victim).unwrap().living.health, 20.0);
    }
}
