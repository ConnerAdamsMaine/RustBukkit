@@
with open("session_orig.rs", "r") as f:
    content = f.read()

# Find and replace the problematic match block
old_block = """                    PlayInbound::TeleportConfirm(id) => {
                        if id != 1 { bail!("invalid teleport acknowledgement"); }
                    }
                    PlayInbound::KeepAlive(received) => {
                        if outstanding.map(|(id, _)| id) != Some(received) {
                            bail!("invalid keepalive reply");
                        }
                        outstanding = None;
                    }
                    PlayInbound::Position { x, y, z, .. } => {
                        *position = PlayerLocation { x, y, z, ..*position };
                        trace!("[MOVE] x={} y={} z={}", position.x, position.y, position.z);
                    }
                    PlayInbound::Look { .. } | PlayInbound::OnGround { .. } => {}
                    _ => {} // Ignore other play packets
++                     PlayInbound::Position { x, y, z, .. } => {
                        *position = PlayerLocation { x, y, z, ..*position };
                        trace!("[MOVE] x={} y={} z={}", position.x, position.y, position.z);
                    }
                    PlayInbound::PositionAndLook { x, y, z, yaw, pitch, on_ground, .. } => {
                        *position = PlayerLocation { x, y, z, yaw, pitch, on_ground, ..*position };
                        trace!("[MOVE] x={} y={} z={} yaw={} pitch={} on_ground={}", position.x, position.y, position.z, position.yaw, position.pitch, position.on_ground);
                    }
                    PlayInbound::Look { yaw, pitch, on_ground } => {
                        position.yaw = yaw;
                        position.pitch = pitch;
                        position.on_ground = on_ground;
                        trace!("[LOOK] yaw={} pitch={} on_ground={}", position.yaw, position.pitch, position.on_ground);
                    }
                    PlayInbound::OnGround(on_ground) => {
                        position.on_ground = on_ground;
                        trace!("[ON_GROUND] on_ground={}", position.on_ground);
                    }
                    PlayInbound::HeldSlot(slot) => {
                        position.selected_hotbar_slot = slot;
                        trace!("[HELD_SLOT] slot={}", position.selected_hotbar_slot);
                    }
                    _ => {} // Ignore other play packets"""

new_block = """                    PlayInbound::TeleportConfirm(id) => {
                        if id != 1 { bail!("invalid teleport acknowledgement"); }
                    }
                    PlayInbound::KeepAlive(received) => {
                        if outstanding.map(|(id, _)| id) != Some(received) {
                            bail!("invalid keepalive reply");
                        }
                        outstanding = None;
                    }
                    PlayInbound::Position { x, y, z, .. } => {
                        *position = PlayerLocation { x, y, z, ..*position };
                        trace!("[MOVE] x={} y={} z={}", position.x, position.y, position.z);
                    }
                    PlayInbound::PositionAndLook { x, y, z, yaw, pitch, on_ground, .. } => {
                        *position = PlayerLocation { x, y, z, yaw, pitch, on_ground, ..*position };
                        trace!("[MOVE] x={} y={} z={} yaw={} pitch={} on_ground={}", position.x, position.y, position.z, position.yaw, position.pitch, position.on_ground);
                    }
                    PlayInbound::Look { yaw, pitch, on_ground } => {
                        position.yaw = yaw;
                        position.pitch = pitch;
                        position.on_ground = on_ground;
                        trace!("[LOOK] yaw={} pitch={} on_ground={}", position.yaw, position.pitch, position.on_ground);
                    }
                    PlayInbound::OnGround(on_ground) => {
                        position.on_ground = on_ground;
                        trace!("[ON_GROUND] on_ground={}", position.on_ground);
                    }
                    PlayInbound::HeldSlot(slot) => {
                        position.selected_hotbar_slot = slot;
                        trace!("[HELD_SLOT] slot={}", position.selected_hotbar_slot);
                    }
                    _ => {} // Ignore other play packets"""

if old_block in content:
    content = content.replace(old_block, new_block)
    with open("session_fixed.rs", "w") as f:
        f.write(content)
    print("Replaced successfully!")
else:
    print("Old block not found!")
    # Try to find where the issue is
    idx = content.find("PlayInbound::TeleportConfirm")
    if idx >= 0:
        print("Found at:", idx)
        print(content[idx:idx+2000])
@@
