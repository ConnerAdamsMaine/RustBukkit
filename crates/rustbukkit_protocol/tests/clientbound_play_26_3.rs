use rustbukkit_chunks::BlockPos;
use rustbukkit_encoding::{MAX_PACKET_LENGTH, encode_frame};
use rustbukkit_protocol::v26_3::clientbound::{
    ClientboundError,
    ClockUpdate,
    CommonSpawnInfo,
    GameMode,
    GlobalPosition,
    PLAY_LOGIN_ID,
    PLAYER_POSITION_ID,
    PlayLogin,
    PlayerPosition,
    RESPAWN_ID,
    SPAWN_POSITION_ID,
    TIME_ID,
    encode_play_login,
    encode_player_position,
    encode_respawn,
    encode_spawn_position,
    encode_time,
};

fn spawn_info() -> CommonSpawnInfo<'static> {
    CommonSpawnInfo {
        dimension_type_id:   0,
        dimension:           "a:",
        seed:                -1,
        game_mode:           GameMode::Survival,
        previous_game_mode:  None,
        is_debug:            false,
        is_flat:             true,
        last_death_location: None,
        portal_cooldown:     0,
        sea_level:           0,
    }
}

fn login<'a>(dimensions: &'a [&'a str], spawn: CommonSpawnInfo<'a>) -> PlayLogin<'a> {
    PlayLogin {
        entity_id: 0,
        hardcore: false,
        dimensions,
        max_players: 0,
        view_distance: 0,
        simulation_distance: 0,
        reduced_debug_info: false,
        show_death_screen: true,
        limited_crafting: false,
        spawn,
        online_mode: false,
        enforces_secure_chat: false,
    }
}

#[test]
fn play_login_encodes_spawn_info_and_online_flags_in_protocol_order() {
    let dimensions = ["overworld", "minecraft:the_nether", "minecraft:the_end"];
    let payload = encode_play_login(PlayLogin {
        entity_id:            0x0102_0304,
        hardcore:             true,
        dimensions:           &dimensions,
        max_players:          300,
        view_distance:        10,
        simulation_distance:  8,
        reduced_debug_info:   true,
        show_death_screen:    false,
        limited_crafting:     true,
        spawn:                CommonSpawnInfo {
            dimension_type_id:   300,
            dimension:           ":overworld",
            seed:                0x0102_0304_0506_0708,
            game_mode:           GameMode::Creative,
            previous_game_mode:  Some(GameMode::Spectator),
            is_debug:            false,
            is_flat:             true,
            last_death_location: Some(GlobalPosition {
                dimension: "the_nether",
                position:  BlockPos::new(-1, 63, 2).unwrap(),
            }),
            portal_cooldown:     300,
            sea_level:           64,
        },
        online_mode:          true,
        enforces_secure_chat: false,
    })
    .unwrap();

    assert_eq!(
        encode_frame(PLAY_LOGIN_ID, &payload).unwrap(),
        [
            0x8e, 0x01, 0x32, 0x01, 0x02, 0x03, 0x04, 0x01, 0x03, 0x13, b'm', b'i', b'n', b'e', b'c', b'r',
            b'a', b'f', b't', b':', b'o', b'v', b'e', b'r', b'w', b'o', b'r', b'l', b'd', 0x14, b'm', b'i',
            b'n', b'e', b'c', b'r', b'a', b'f', b't', b':', b't', b'h', b'e', b'_', b'n', b'e', b't', b'h',
            b'e', b'r', 0x11, b'm', b'i', b'n', b'e', b'c', b'r', b'a', b'f', b't', b':', b't', b'h', b'e',
            b'_', b'e', b'n', b'd', 0xac, 0x02, 0x0a, 0x08, 0x01, 0x00, 0x01, 0xac, 0x02, 0x13, b'm', b'i',
            b'n', b'e', b'c', b'r', b'a', b'f', b't', b':', b'o', b'v', b'e', b'r', b'w', b'o', b'r', b'l',
            b'd', 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x01, 0x04, 0x00, 0x01, 0x01, 0x14, b'm',
            b'i', b'n', b'e', b'c', b'r', b'a', b'f', b't', b':', b't', b'h', b'e', b'_', b'n', b'e', b't',
            b'h', b'e', b'r', 0xff, 0xff, 0xff, 0xc0, 0x00, 0x00, 0x20, 0x3f, 0xac, 0x02, 0x40, 0x01, 0x00,
        ]
    );
}

#[test]
fn respawn_encodes_absent_optionals_and_data_to_keep() {
    let payload = encode_respawn(
        CommonSpawnInfo {
            dimension: "overworld",
            sea_level: 63,
            ..spawn_info()
        },
        3,
    )
    .unwrap();
    assert_eq!(
        encode_frame(RESPAWN_ID, &payload).unwrap(),
        [
            0x26, 0x54, 0x00, 0x13, b'm', b'i', b'n', b'e', b'c', b'r', b'a', b'f', b't', b':', b'o', b'v',
            b'e', b'r', b'w', b'o', b'r', b'l', b'd', 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00,
            0x00, 0x00, 0x01, 0x00, 0x00, 0x3f, 0x03,
        ]
    );
}

#[test]
fn spawn_info_rejects_invalid_holder_identifiers_and_death_positions() {
    assert!(matches!(
        encode_respawn(
            CommonSpawnInfo {
                dimension_type_id: -1,
                ..spawn_info()
            },
            0
        ),
        Err(ClientboundError::InvalidDimensionTypeId(-1))
    ));
    assert!(matches!(
        encode_respawn(
            CommonSpawnInfo {
                dimension: "minecraft:Invalid",
                ..spawn_info()
            },
            0
        ),
        Err(ClientboundError::InvalidDimension)
    ));
    assert!(matches!(
        encode_respawn(
            CommonSpawnInfo {
                last_death_location: Some(GlobalPosition {
                    dimension: "minecraft:overworld",
                    position:  BlockPos {
                        x: 33_554_432,
                        y: 0,
                        z: 0,
                    },
                }),
                ..spawn_info()
            },
            0
        ),
        Err(ClientboundError::InvalidPosition)
    ));
    assert!(matches!(
        encode_respawn(
            CommonSpawnInfo {
                last_death_location: Some(GlobalPosition {
                    dimension: "bad namespace:path",
                    position:  BlockPos::new(0, 0, 0).unwrap(),
                }),
                ..spawn_info()
            },
            0
        ),
        Err(ClientboundError::InvalidDimension)
    ));
}

#[test]
fn play_login_rejects_equivalent_dimension_keys() {
    let dimensions = ["overworld", ":overworld"];
    assert!(matches!(
        encode_play_login(login(&dimensions, spawn_info())),
        Err(ClientboundError::DuplicateDimension)
    ));
}

#[test]
fn play_login_accepts_maximum_packet_body_and_rejects_one_more_byte() {
    let mut keys: Vec<String> = (0..63)
        .map(|index| format!("a:{index:02}{}", "x".repeat(32_763)))
        .collect();
    keys.push(format!("a:{}", "x".repeat(32_602)));
    let dimensions: Vec<&str> = keys.iter().map(String::as_str).collect();
    let payload = encode_play_login(login(&dimensions, spawn_info())).unwrap();
    assert_eq!(payload.len() + 1, MAX_PACKET_LENGTH);
    assert_eq!(encode_frame(PLAY_LOGIN_ID, &payload).unwrap().len(), MAX_PACKET_LENGTH + 3);

    keys.last_mut().unwrap().push('x');
    let dimensions: Vec<&str> = keys.iter().map(String::as_str).collect();
    assert!(matches!(
        encode_play_login(login(&dimensions, spawn_info())),
        Err(ClientboundError::PacketTooLarge)
    ));
}

#[test]
fn player_position_includes_delta_velocity_and_integer_relative_flags() {
    let payload = encode_player_position(PlayerPosition {
        teleport_id:    300,
        position:       [1.5, -2.0, 3.5],
        delta:          [0.25, -0.5, 1.0],
        yaw:            -90.0,
        pitch:          45.0,
        relative_flags: 0x185,
    });

    assert_eq!(
        encode_frame(PLAYER_POSITION_ID, &payload).unwrap(),
        [
            0x3f, 0x49, 0xac, 0x02, 0x3f, 0xf8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x40, 0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3f, 0xd0, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0xbf, 0xe0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3f, 0xf0, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0xc2, 0xb4, 0x00, 0x00, 0x42, 0x34, 0x00, 0x00, 0x00, 0x00, 0x01, 0x85,
        ]
    );
}

#[test]
fn spawn_position_encodes_canonical_dimension_position_and_both_angles() {
    let payload = encode_spawn_position("overworld", BlockPos::new(-1, 63, 2).unwrap(), -90.0, 45.0).unwrap();
    assert_eq!(
        encode_frame(SPAWN_POSITION_ID, &payload).unwrap(),
        [
            0x25, 0x63, 0x13, b'm', b'i', b'n', b'e', b'c', b'r', b'a', b'f', b't', b':', b'o', b'v', b'e',
            b'r', b'w', b'o', b'r', b'l', b'd', 0xff, 0xff, 0xff, 0xc0, 0x00, 0x00, 0x20, 0x3f, 0xc2, 0xb4,
            0x00, 0x00, 0x42, 0x34, 0x00, 0x00,
        ]
    );
}

#[test]
fn spawn_position_rejects_invalid_or_oversized_dimension_identifiers() {
    let pos = BlockPos::new(0, 64, 0).unwrap();
    assert!(matches!(
        encode_spawn_position("minecraft:Invalid", pos, 0.0, 0.0),
        Err(ClientboundError::InvalidDimension)
    ));
    assert!(matches!(
        encode_spawn_position(&"a".repeat(32_768), pos, 0.0, 0.0),
        Err(ClientboundError::InvalidDimension)
    ));
}

#[test]
fn spawn_position_rejects_coordinates_outside_the_packed_position_range() {
    let pos = BlockPos {
        x: 33_554_432,
        y: 64,
        z: 0,
    };
    assert!(matches!(
        encode_spawn_position("minecraft:overworld", pos, 0.0, 0.0),
        Err(ClientboundError::InvalidPosition)
    ));
}

#[test]
fn time_updates_encode_registry_ids_varlong_ticks_and_clock_rates() {
    let payload = encode_time(
        1,
        &[
            ClockUpdate {
                registry_id:  300,
                ticks:        -1,
                partial_tick: 0.5,
                rate:         1.0,
            },
            ClockUpdate {
                registry_id:  0,
                ticks:        300,
                partial_tick: 0.0,
                rate:         0.25,
            },
        ],
    )
    .unwrap();
    assert_eq!(
        encode_frame(TIME_ID, &payload).unwrap(),
        [
            0x29, 0x73, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0xac, 0x02, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01, 0x3f, 0x00, 0x00, 0x00, 0x3f, 0x80, 0x00, 0x00, 0x00,
            0xac, 0x02, 0x00, 0x00, 0x00, 0x00, 0x3e, 0x80, 0x00, 0x00,
        ]
    );
}

#[test]
fn time_update_allows_an_empty_clock_map() {
    let payload = encode_time(-1, &[]).unwrap();
    assert_eq!(
        encode_frame(TIME_ID, &payload).unwrap(),
        [0x0a, 0x73, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00]
    );
}

#[test]
fn time_update_rejects_negative_or_duplicate_clock_registry_ids() {
    let clock = ClockUpdate {
        registry_id:  3,
        ticks:        0,
        partial_tick: 0.0,
        rate:         1.0,
    };
    assert!(matches!(encode_time(0, &[clock, clock]), Err(ClientboundError::DuplicateClockId(3))));
    assert!(matches!(
        encode_time(
            0,
            &[ClockUpdate {
                registry_id: -1,
                ..clock
            }]
        ),
        Err(ClientboundError::InvalidClockId(-1))
    ));
}

#[test]
fn time_update_accepts_the_maximum_frame_size_and_rejects_one_byte_more() {
    let target = MAX_PACKET_LENGTH - 1;
    let mut updates = Vec::new();
    let mut entry_bytes = 0;
    for id in 0..i32::MAX {
        let id_len = if id < 128 {
            1
        } else if id < 16_384 {
            2
        } else {
            3
        };
        let next_count = updates.len() + 1;
        let count_len = if next_count < 128 {
            1
        } else if next_count < 16_384 {
            2
        } else {
            3
        };
        let next_entry = id_len + 1 + 8;
        if 8 + count_len + entry_bytes + next_entry > target {
            break;
        }
        updates.push(ClockUpdate {
            registry_id:  id,
            ticks:        0,
            partial_tick: 0.0,
            rate:         1.0,
        });
        entry_bytes += next_entry;
    }

    let mut padding = target - (8 + 3 + entry_bytes);
    for update in &mut updates {
        let extra = padding.min(9);
        if extra != 0 {
            update.ticks = (1_u64 << (7 * extra)) as i64;
            padding -= extra;
        }
        if padding == 0 {
            break;
        }
    }
    assert_eq!(padding, 0);

    let payload = encode_time(0, &updates).unwrap();
    assert_eq!(payload.len() + 1, MAX_PACKET_LENGTH);
    assert_eq!(encode_frame(TIME_ID, &payload).unwrap().len(), MAX_PACKET_LENGTH + 3);

    updates.last_mut().unwrap().ticks = 128;
    assert!(matches!(encode_time(0, &updates), Err(ClientboundError::PacketTooLarge)));
}
