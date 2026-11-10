use std::collections::{BTreeMap, BTreeSet};

use rustbukkit_chunks::BlockPos;
use rustbukkit_decoding::{Reader, WireError};
use rustbukkit_inventory::InventoryAction;
use thiserror::Error;

use crate::{BlockFace, DigAction, PlayInbound};

// 26.3 serverbound play packet IDs, from the generated `packets.json` report
// (`registry_data/26.3/packet_ids.json`); asserted by `tests/protocol_ids_26_3.rs`.
pub const ACCEPT_TELEPORTATION_ID: i32 = 0;
pub const ATTACK_ID: i32 = 1;
pub const CLIENT_COMMAND_ID: i32 = 12;
pub const CONTAINER_CLICK_ID: i32 = 18;
pub const KEEP_ALIVE_ID: i32 = 28;
pub const MOVE_PLAYER_POS_ID: i32 = 30;
pub const MOVE_PLAYER_POS_ROT_ID: i32 = 31;
pub const MOVE_PLAYER_ROT_ID: i32 = 32;
pub const MOVE_PLAYER_STATUS_ID: i32 = 33;
pub const PLAYER_ACTION_ID: i32 = 41;
pub const SET_CARRIED_ITEM_ID: i32 = 54;
pub const USE_ITEM_ON_ID: i32 = 66;
pub const USE_ITEM_ID: i32 = 67;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hand {
    Main,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientCommand {
    PerformRespawn,
    RequestStats,
    RequestGameruleValues,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerInput {
    Pickup,
    QuickMove,
    Swap,
    Clone,
    Throw,
    QuickCraft,
    PickupAll,
}

/// A client-provided prediction, not authoritative inventory contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HashedItem {
    pub item_id:            i32,
    pub count:              i32,
    pub added_components:   BTreeMap<i32, i32>,
    pub removed_components: BTreeSet<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerAction {
    StartDestroyBlock,
    ChangeDestroyDirection,
    AbortDestroyBlock,
    StopDestroyBlock,
    DropAllItems,
    DropItem,
    ReleaseUseItem,
    SwapItemWithOffhand,
    Stab,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MovementFlags {
    pub on_ground:            bool,
    pub horizontal_collision: bool,
}

#[derive(Debug, PartialEq)]
pub enum ServerboundPlay {
    TeleportConfirm {
        id:    i32,
        x:     f64,
        y:     f64,
        z:     f64,
        yaw:   f32,
        pitch: f32,
    },
    Attack {
        target_entity_id: i32,
    },
    ClientCommand(ClientCommand),
    ContainerClick {
        container_id:  i32,
        state_id:      i32,
        slot:          i16,
        button:        i8,
        input:         ContainerInput,
        changed_slots: BTreeMap<i16, Option<HashedItem>>,
        carried:       Option<HashedItem>,
    },
    KeepAlive(i64),
    PlayerAction {
        action:    PlayerAction,
        position:  (i32, i32, i32),
        direction: BlockFace,
        sequence:  i32,
    },
    HeldSlot(i16),
    UseItemOn {
        hand:             Hand,
        position:         (i32, i32, i32),
        direction:        BlockFace,
        hit_offset:       (f32, f32, f32),
        inside_block:     bool,
        world_border_hit: bool,
        sequence:         i32,
    },
    UseItem {
        hand:     Hand,
        sequence: i32,
        yaw:      f32,
        pitch:    f32,
    },
    Position {
        x:     f64,
        y:     f64,
        z:     f64,
        flags: MovementFlags,
    },
    PositionAndLook {
        x:     f64,
        y:     f64,
        z:     f64,
        yaw:   f32,
        pitch: f32,
        flags: MovementFlags,
    },
    Look {
        yaw:   f32,
        pitch: f32,
        flags: MovementFlags,
    },
    MovementStatus(MovementFlags),
}

impl From<ServerboundPlay> for PlayInbound {
    fn from(packet: ServerboundPlay) -> Self {
        match packet {
            ServerboundPlay::TeleportConfirm { id, .. } => PlayInbound::TeleportConfirm(id),
            ServerboundPlay::KeepAlive(id) => PlayInbound::KeepAlive(id),
            ServerboundPlay::Position { x, y, z, .. } => PlayInbound::Position { x, y, z },
            ServerboundPlay::PositionAndLook {
                x,
                y,
                z,
                yaw,
                pitch,
                flags,
            } => {
                PlayInbound::PositionAndLook {
                    x,
                    y,
                    z,
                    yaw,
                    pitch,
                    on_ground: flags.on_ground,
                }
            }
            ServerboundPlay::Look { yaw, pitch, flags } => {
                PlayInbound::Look {
                    yaw,
                    pitch,
                    on_ground: flags.on_ground,
                }
            }
            ServerboundPlay::MovementStatus(flags) => PlayInbound::OnGround(flags.on_ground),

            ServerboundPlay::PlayerAction {
                action,
                position,
                direction: _direction,
                sequence,
            } => {
                let Some(block) = BlockPos::new(position.0, position.1, position.2) else {
                    // Out-of-range coordinates are a client error, not a panic:
                    // surface them as an ignored play packet.
                    return PlayInbound::Other(41);
                };
                let dig_action = match action {
                    PlayerAction::StartDestroyBlock => DigAction::Start,
                    PlayerAction::ChangeDestroyDirection => DigAction::Cancel,
                    PlayerAction::AbortDestroyBlock => DigAction::Cancel,
                    PlayerAction::StopDestroyBlock => DigAction::Finish,
                    PlayerAction::DropAllItems => DigAction::DropStack,
                    PlayerAction::DropItem => DigAction::DropOne,
                    PlayerAction::ReleaseUseItem => DigAction::Cancel,
                    PlayerAction::SwapItemWithOffhand => DigAction::Cancel,
                    PlayerAction::Stab => DigAction::Cancel,
                };
                PlayInbound::Dig {
                    action: dig_action,
                    position: block,
                    sequence,
                }
            }
            ServerboundPlay::HeldSlot(slot) => PlayInbound::HeldSlot(slot as u8),
            ServerboundPlay::UseItemOn {
                hand,
                position,
                direction,
                sequence,
                ..
            } => {
                match BlockPos::new(position.0, position.1, position.2) {
                    Some(target) => {
                        PlayInbound::Place {
                            target,
                            face: direction,
                            main_hand: matches!(hand, Hand::Main),
                            sequence,
                        }
                    }
                    None => PlayInbound::Other(66),
                }
            }
            ServerboundPlay::UseItem {
                hand,
                sequence,
                yaw,
                pitch,
            } => {
                PlayInbound::UseItem {
                    hand,
                    sequence,
                    yaw,
                    pitch,
                }
            }
            ServerboundPlay::Attack { target_entity_id } => {
                PlayInbound::Attack {
                    target_entity_id,
                    sneaking: false,
                }
            }
            ServerboundPlay::ClientCommand(_) => PlayInbound::RespawnRequest,
            ServerboundPlay::ContainerClick {
                container_id: _,
                state_id,
                slot,
                button,
                input,
                changed_slots,
                carried: _,
            } => {
                let action = match (input, slot, button) {
                    (ContainerInput::QuickMove, 9..=45, 0) => InventoryAction::DropOne(slot as u8),
                    (ContainerInput::QuickMove, 9..=45, 1) => InventoryAction::DropStack(slot as u8),
                    (ContainerInput::Pickup, -999, 0) => InventoryAction::DropCursorStack,
                    (ContainerInput::Pickup, -999, 1) => InventoryAction::DropCursorOne,
                    _ => InventoryAction::Rearrange,
                };
                let mut changes = Vec::new();
                for (s, _item) in changed_slots {
                    changes.push((s as u8, None)); // Simplified - would need proper conversion
                }
                PlayInbound::ClickContainer {
                    state_id,
                    action,
                    changes,
                    carried: None,
                }
            }
        }
    }
}
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PlayPacketError {
    #[error(transparent)]
    Wire(#[from] WireError),
    #[error("trailing bytes in serverbound play packet")]
    TrailingBytes,
    #[error("invalid player action {0}")]
    InvalidAction(i32),
    #[error("invalid client command {0}")]
    InvalidClientCommand(i32),
    #[error("{name} length {count} exceeds its allowed range 0..={max}")]
    InvalidCollectionLength {
        name:  &'static str,
        count: i32,
        max:   usize,
    },
    #[error("invalid negative registry ID {0}")]
    InvalidRegistryId(i32),
    #[error("unsupported serverbound play packet {0}")]
    Unsupported(i32),
}

fn read_count(
    reader: &mut Reader<'_>,
    name: &'static str,
    max: usize,
    min_entry_bytes: usize,
) -> Result<usize, PlayPacketError> {
    let count = reader.varint()?;
    if count < 0 || count as usize > max {
        return Err(PlayPacketError::InvalidCollectionLength { name, count, max });
    }
    let count = count as usize;
    if count > reader.remaining() / min_entry_bytes {
        return Err(WireError::Truncated.into());
    }
    Ok(count)
}

fn read_registry_id(reader: &mut Reader<'_>) -> Result<i32, PlayPacketError> {
    let id = reader.varint()?;
    if id < 0 {
        return Err(PlayPacketError::InvalidRegistryId(id));
    }
    Ok(id)
}

fn read_hashed_item(reader: &mut Reader<'_>) -> Result<Option<HashedItem>, PlayPacketError> {
    if !super::read_boolean(reader)? {
        return Ok(None);
    }
    let item_id = read_registry_id(reader)?;
    let count = reader.varint()?;
    let added_count = read_count(reader, "added component", 256, 5)?;
    let mut added_components = BTreeMap::new();
    for _ in 0..added_count {
        added_components.insert(read_registry_id(reader)?, reader.i32()?);
    }
    let removed_count = read_count(reader, "removed component", 256, 1)?;
    let mut removed_components = BTreeSet::new();
    for _ in 0..removed_count {
        removed_components.insert(read_registry_id(reader)?);
    }
    Ok(Some(HashedItem {
        item_id,
        count,
        added_components,
        removed_components,
    }))
}

fn read_movement_flags(reader: &mut Reader<'_>) -> Result<MovementFlags, WireError> {
    let flags = reader.u8()?;
    Ok(MovementFlags {
        on_ground:            flags & 1 != 0,
        horizontal_collision: flags & 2 != 0,
    })
}

fn read_hand(reader: &mut Reader<'_>) -> Result<Hand, WireError> {
    Ok(match reader.varint()? {
        1 => Hand::Off,
        _ => Hand::Main,
    })
}

fn direction_by_id(id: i32) -> BlockFace {
    match id.rem_euclid(6) {
        0 => BlockFace::Down,
        1 => BlockFace::Up,
        2 => BlockFace::North,
        3 => BlockFace::South,
        4 => BlockFace::West,
        _ => BlockFace::East,
    }
}

pub fn parse_play_packet(id: i32, payload: &[u8]) -> Result<ServerboundPlay, PlayPacketError> {
    let mut reader = Reader::new(payload);
    let packet = match id {
        ACCEPT_TELEPORTATION_ID => {
            ServerboundPlay::TeleportConfirm {
                id:    reader.varint()?,
                x:     reader.f64()?,
                y:     reader.f64()?,
                z:     reader.f64()?,
                yaw:   reader.f32()?,
                pitch: reader.f32()?,
            }
        }
        ATTACK_ID => {
            ServerboundPlay::Attack {
                target_entity_id: reader.varint()?,
            }
        }
        CLIENT_COMMAND_ID => {
            ServerboundPlay::ClientCommand(match reader.varint()? {
                0 => ClientCommand::PerformRespawn,
                1 => ClientCommand::RequestStats,
                2 => ClientCommand::RequestGameruleValues,
                other => return Err(PlayPacketError::InvalidClientCommand(other)),
            })
        }
        CONTAINER_CLICK_ID => {
            let container_id = reader.varint()?;
            let state_id = reader.varint()?;
            let slot = reader.i16()?;
            let button = reader.i8()?;
            let input = match reader.varint()? {
                1 => ContainerInput::QuickMove,
                2 => ContainerInput::Swap,
                3 => ContainerInput::Clone,
                4 => ContainerInput::Throw,
                5 => ContainerInput::QuickCraft,
                6 => ContainerInput::PickupAll,
                _ => ContainerInput::Pickup,
            };
            let changed_count = read_count(&mut reader, "changed slot", 128, 3)?;
            let mut changed_slots = BTreeMap::new();
            for _ in 0..changed_count {
                changed_slots.insert(reader.i16()?, read_hashed_item(&mut reader)?);
            }
            ServerboundPlay::ContainerClick {
                container_id,
                state_id,
                slot,
                button,
                input,
                changed_slots,
                carried: read_hashed_item(&mut reader)?,
            }
        }
        KEEP_ALIVE_ID => ServerboundPlay::KeepAlive(reader.i64()?),
        MOVE_PLAYER_POS_ID => {
            ServerboundPlay::Position {
                x:     reader.f64()?,
                y:     reader.f64()?,
                z:     reader.f64()?,
                flags: read_movement_flags(&mut reader)?,
            }
        }
        MOVE_PLAYER_POS_ROT_ID => {
            ServerboundPlay::PositionAndLook {
                x:     reader.f64()?,
                y:     reader.f64()?,
                z:     reader.f64()?,
                yaw:   reader.f32()?,
                pitch: reader.f32()?,
                flags: read_movement_flags(&mut reader)?,
            }
        }
        MOVE_PLAYER_ROT_ID => {
            ServerboundPlay::Look {
                yaw:   reader.f32()?,
                pitch: reader.f32()?,
                flags: read_movement_flags(&mut reader)?,
            }
        }
        MOVE_PLAYER_STATUS_ID => ServerboundPlay::MovementStatus(read_movement_flags(&mut reader)?),
        PLAYER_ACTION_ID => {
            let action = match reader.varint()? {
                0 => PlayerAction::StartDestroyBlock,
                1 => PlayerAction::ChangeDestroyDirection,
                2 => PlayerAction::AbortDestroyBlock,
                3 => PlayerAction::StopDestroyBlock,
                4 => PlayerAction::DropAllItems,
                5 => PlayerAction::DropItem,
                6 => PlayerAction::ReleaseUseItem,
                7 => PlayerAction::SwapItemWithOffhand,
                8 => PlayerAction::Stab,
                other => return Err(PlayPacketError::InvalidAction(other)),
            };
            let position = reader.position()?;
            let direction = direction_by_id(i32::from(reader.u8()?));
            ServerboundPlay::PlayerAction {
                action,
                position,
                direction,
                sequence: reader.varint()?,
            }
        }
        SET_CARRIED_ITEM_ID => ServerboundPlay::HeldSlot(reader.i16()?),
        USE_ITEM_ON_ID => {
            ServerboundPlay::UseItemOn {
                hand:             read_hand(&mut reader)?,
                position:         reader.position()?,
                direction:        direction_by_id(reader.varint()?),
                hit_offset:       (reader.f32()?, reader.f32()?, reader.f32()?),
                inside_block:     super::read_boolean(&mut reader)?,
                world_border_hit: super::read_boolean(&mut reader)?,
                sequence:         reader.varint()?,
            }
        }
        USE_ITEM_ID => {
            ServerboundPlay::UseItem {
                hand:     read_hand(&mut reader)?,
                sequence: reader.varint()?,
                yaw:      reader.f32()?,
                pitch:    reader.f32()?,
            }
        }
        _ => return Err(PlayPacketError::Unsupported(id)),
    };
    if reader.remaining() != 0 {
        return Err(PlayPacketError::TrailingBytes);
    }
    Ok(packet)
}
