$content = Get-Content 'crates/rustbukkit_bin/src/session.rs' -Raw

# 1. Add imports after world_persistence
$content = $content -replace 'use crate::world_persistence::WorldStore;\n', 'use crate::world_persistence::WorldStore;\nuse rustbukkit_chunks::{BlockKind, BlockPos, World};\nuse rustbukkit_protocol::encode_block_update;\n'

# 2. Add the handle_block_change function before offline_uuid
$handle_block_change = @'
/// Sends a block update packet to the client and broadcasts a block event
async fn handle_block_change(
    connection: &mut Connection,
    version: 'static dyn VersionPack,
    world: &World,
    session_registry: &SessionRegistry,
    block_events: &broadcast::Sender<BlockEvent>,
    position: &PlayerLocation,
    block_pos: BlockPos,
    new_block: BlockKind,
    session_uuid: Uuid,
) -> Result<()> {
    // Get the current block state ID
    let current_block = world.get_block(block_pos);
    let block_state_id = match new_block {
        BlockKind::Air => 0,
        BlockKind::Bedrock => 3,
        BlockKind::Dirt => 4,
        BlockKind::GrassBlock => 5,
        BlockKind::Sand => 6,
        BlockKind::Stone => 1,
        BlockKind::Cobblestone => 7,
        BlockKind::Water => 2,
    };

    // Apply the block change atomically
    if world.set_block_if(block_pos, current_block, new_block) {
        // Send BlockUpdate to the player
        if let Ok(payload) = encode_block_update(block_pos, block_state_id) {
            connection
                .write_frame(version.clientbound_id(ClientboundPacket::BlockUpdate), &payload)
                .await?;
        }

        // Broadcast block event to other players
        let event = match new_block {
            BlockKind::Air => BlockEvent::Broken {
                position: (block_pos.x, block_pos.y, block_pos.z),
                entity_id: None,
            },
            _ => BlockEvent::Placed {
                position: (block_pos.x, block_pos.y, block_pos.z),
                block: block_state_id,
                entity_id: None,
            },
        };
        let _ = block_events.send(event);
    }

    Ok(())
}

'@

$content = $content -replace 'fn offline_uuid\(username: &str\) -> Uuid \{', $handle_block_change + 'fn offline_uuid(username: &str) -> Uuid {'

# 3. Update the play function call in negotiate_configuration to pass world
$content = $content -replace 'play\(connection, version, &mut location, shutdown\).await', 'play(connection, version, &mut location, &world, &registry, &block_events, session_uuid, shutdown).await'

# 4. Update the play function signature
$old_play = @'
async fn play(
    connection: &mut Connection,
    version: 'static dyn VersionPack,
    position: &mut PlayerLocation,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
'@

$new_play = @'
async fn play(
    connection: &mut Connection,
    version: 'static dyn VersionPack,
    position: &mut PlayerLocation,
    world: &World,
    session_registry: &SessionRegistry,
    block_events: &broadcast::Sender<BlockEvent>,
    session_uuid: Uuid,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
'@

$content = $content -replace $old_play, $new_play

# 5. Add Dig and Place handling in the match statement
$old_match_end = @'
                    PlayInbound::HeldSlot(slot) => {
                        position.selected_hotbar_slot = slot;
                        trace!("[HELD_SLOT] slot={}", position.selected_hotbar_slot);
                    }
                    _ => {} // Ignore other play packets
                }
'@

$new_match_end = @'
                    PlayInbound::HeldSlot(slot) => {
                        position.selected_hotbar_slot = slot;
                        trace!("[HELD_SLOT] slot={}", position.selected_hotbar_slot);
                    }
                    PlayInbound::Dig { action, position: block_pos, sequence: _ } => {
                        trace!("[DIG] action={:?} pos=({},{},{})", action, block_pos.x, block_pos.y, block_pos.z);
                        match action {
                            DigAction::Finish => {
                                // Break block - set to air
                                handle_block_change(
                                    connection,
                                    version,
                                    world,
                                    session_registry,
                                    block_events,
                                    position,
                                    block_pos,
                                    BlockKind::Air,
                                    session_uuid,
                                ).await?;
                            }
                            DigAction::Start | DigAction::Cancel | DigAction::DropStack | DigAction::DropOne => {
                                // For now, just acknowledge - no block change
                            }
                        }
                    }
                    PlayInbound::Place { target, face, main_hand: _, sequence: _ } => {
                        trace!("[PLACE] target=({},{},{}) face={:?}", target.x, target.y, target.z, face);
                        // Calculate the position to place the block (adjacent to target on the face)
                        let (dx, dy, dz) = match face {
                            BlockFace::Down => (0, -1, 0),
                            BlockFace::Up => (0, 1, 0),
                            BlockFace::North => (0, 0, -1),
                            BlockFace::South => (0, 0, 1),
                            BlockFace::West => (-1, 0, 0),
                            BlockFace::East => (1, 0, 0),
                        };
                        let place_pos = BlockPos::new(
                            target.x + dx,
                            target.y + dy,
                            target.z + dz,
                        );
                        if let Some(place_pos) = place_pos {
                            // Check if the target position is air (can place)
                            if world.get_block(place_pos) == BlockKind::Air {
                                // Place a grass block for now (using held item would be better)
                                handle_block_change(
                                    connection,
                                    version,
                                    world,
                                    session_registry,
                                    block_events,
                                    position,
                                    place_pos,
                                    BlockKind::GrassBlock,
                                    session_uuid,
                                ).await?;
                            }
                        }
                    }
                    _ => {} // Ignore other play packets
                }
'@

$content = $content -replace $old_match_end, $new_match_end

[System.IO.File]::WriteAllText('crates/rustbukkit_bin/src/session.rs', $content)
Write-Host 'All changes applied'
