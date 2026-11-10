use rustbukkit_protocol::BlockFace;
use rustbukkit_protocol::v26_3::play::{
    ClientCommand,
    ContainerInput,
    Hand,
    HashedItem,
    MovementFlags,
    PlayPacketError,
    PlayerAction,
    ServerboundPlay,
    parse_play_packet,
};

const POSITION: [u8; 24] = [
    0x3f, 0xf8, 0, 0, 0, 0, 0, 0, // 1.5
    0xc0, 0, 0, 0, 0, 0, 0, 0, // -2.0
    0x40, 0x0c, 0, 0, 0, 0, 0, 0, // 3.5
];

const USE_ITEM_ON: [u8; 30] = [
    1, // off hand
    0xff, 0xff, 0xff, 0xc0, 0, 0, 0x20, 0x3f, // (-1, 63, 2)
    0xff, 0xff, 0xff, 0xff, 0x0f, // direction -1, wrapped to east
    0x3e, 0x80, 0, 0, // x offset 0.25
    0x3f, 0, 0, 0, // y offset 0.5
    0x3f, 0x40, 0, 0, // z offset 0.75
    1, 0, // inside block, not world-border hit
    0xac, 0x02, // sequence 300
];

const CONTAINER_CLICK: &[u8] = &[
    0xac, 0x02, // container ID 300
    7,    // state ID
    0xfc, 0x19, // slot -999
    0xff, // button -1
    5,    // quick craft
    2,    // changed slots
    0xff, 0xff, 0, // slot -1: empty stack
    0, 5, 1, // slot 5: present stack
    0xac, 0x02, 64, // item ID 300, count 64
    2,  // added component hashes
    0, 0x12, 0x34, 0x56, 0x78, // component 0, hash 0x12345678
    0xac, 0x02, 0xff, 0xff, 0xff, 0xff, // component 300, hash -1
    3, 17, 0, 0x80, 0x80, 1, // removed components 17, 0, 16384
    0xfe, 3, 2, 0, 0, // carried: nonzero presence, item ID 3, count 2, empty patch
];

#[test]
fn container_click_uses_26_3_packet_id_instead_of_container_button_id() {
    assert!(matches!(parse_play_packet(18, CONTAINER_CLICK), Ok(ServerboundPlay::ContainerClick { .. })));
    assert_eq!(parse_play_packet(17, CONTAINER_CLICK), Err(PlayPacketError::Unsupported(17)));
}

#[test]
fn container_click_preserves_hashed_predictions_and_signed_fields() {
    assert_eq!(
        parse_play_packet(18, CONTAINER_CLICK).unwrap(),
        ServerboundPlay::ContainerClick {
            container_id:  300,
            state_id:      7,
            slot:          -999,
            button:        -1,
            input:         ContainerInput::QuickCraft,
            changed_slots: [
                (-1, None),
                (
                    5,
                    Some(HashedItem {
                        item_id:            300,
                        count:              64,
                        added_components:   [(0, 0x1234_5678), (300, -1)].into_iter().collect(),
                        removed_components: [17, 0, 16384].into_iter().collect(),
                    }),
                ),
            ]
            .into_iter()
            .collect(),
            carried:       Some(HashedItem {
                item_id:            3,
                count:              2,
                added_components:   Default::default(),
                removed_components: Default::default(),
            }),
        }
    );
}

#[test]
fn container_click_input_uses_varint_and_out_of_bounds_pickup() {
    let header = [0, 0, 0, 0, 0];
    for (encoded, expected) in [
        (&[0][..], ContainerInput::Pickup),
        (&[1][..], ContainerInput::QuickMove),
        (&[2][..], ContainerInput::Swap),
        (&[3][..], ContainerInput::Clone),
        (&[4][..], ContainerInput::Throw),
        (&[5][..], ContainerInput::QuickCraft),
        (&[6][..], ContainerInput::PickupAll),
        (&[7][..], ContainerInput::Pickup),
        (&[0x80, 1][..], ContainerInput::Pickup),
        (&[0xff, 0xff, 0xff, 0xff, 0x0f][..], ContainerInput::Pickup),
    ] {
        let mut payload = header.to_vec();
        payload.extend_from_slice(encoded);
        payload.extend_from_slice(&[0, 0]);
        let ServerboundPlay::ContainerClick { input, .. } = parse_play_packet(18, &payload).unwrap() else {
            panic!("expected container click");
        };
        assert_eq!(input, expected);
    }
}

#[test]
fn container_click_uses_last_duplicate_map_values_and_deduplicates_removed_components() {
    let payload = [
        0, 0, 0, 0, 0, 0, // header
        2, 0, 4, 1, 1, 1, 0, 0, // slot 4: item 1, empty patch
        0, 4, 0, // same slot: empty, replaces the previous item
        1, 3, 2, // carried item 3, count 2
        2, 5, 0, 0, 0, 1, 5, 0, 0, 0, 2, // duplicate added component 5
        2, 6, 6, // duplicate removed component 6
    ];
    let ServerboundPlay::ContainerClick {
        changed_slots,
        carried,
        ..
    } = parse_play_packet(18, &payload).unwrap()
    else {
        panic!("expected container click");
    };
    assert_eq!(changed_slots, [(4, None)].into_iter().collect());
    assert_eq!(
        carried,
        Some(HashedItem {
            item_id:            3,
            count:              2,
            added_components:   [(5, 2)].into_iter().collect(),
            removed_components: [6].into_iter().collect(),
        })
    );
}

#[test]
fn container_click_enforces_changed_slot_and_component_limits() {
    let header = [0, 0, 0, 0, 0, 0];
    let mut max_slots = header.to_vec();
    max_slots.extend_from_slice(&[0x80, 1]);
    for slot in 0i16..128 {
        max_slots.extend_from_slice(&slot.to_be_bytes());
        max_slots.push(0);
    }
    max_slots.push(0);
    let ServerboundPlay::ContainerClick { changed_slots, .. } = parse_play_packet(18, &max_slots).unwrap()
    else {
        panic!("expected container click");
    };
    assert_eq!(changed_slots.len(), 128);

    for (encoded, count) in [(&[0x81, 1][..], 129), (&[0xff, 0xff, 0xff, 0xff, 0x0f][..], -1)] {
        let mut payload = header.to_vec();
        payload.extend_from_slice(encoded);
        assert_eq!(
            parse_play_packet(18, &payload),
            Err(PlayPacketError::InvalidCollectionLength {
                name: "changed slot",
                count,
                max: 128,
            })
        );
    }

    let mut max_components = header.to_vec();
    max_components.extend_from_slice(&[0, 1, 1, 1, 0x80, 2]);
    for _ in 0..256 {
        max_components.extend_from_slice(&[0, 0, 0, 0, 1]);
    }
    max_components.extend_from_slice(&[0x80, 2]);
    max_components.extend_from_slice(&[0; 256]);
    assert!(parse_play_packet(18, &max_components).is_ok());

    for (prefix, name) in [
        (&[0, 1, 1, 1][..], "added component"),
        (&[0, 1, 1, 1, 0][..], "removed component"),
    ] {
        for (encoded, count) in [(&[0x81, 2][..], 257), (&[0xff, 0xff, 0xff, 0xff, 0x0f][..], -1)] {
            let mut payload = header.to_vec();
            payload.extend_from_slice(prefix);
            payload.extend_from_slice(encoded);
            assert_eq!(
                parse_play_packet(18, &payload),
                Err(PlayPacketError::InvalidCollectionLength {
                    name,
                    count,
                    max: 256,
                })
            );
        }
    }
}

#[test]
fn container_click_rejects_negative_registry_ids_truncation_and_trailing_data() {
    let negative_id = [0xff, 0xff, 0xff, 0xff, 0x0f];
    for prefix in [
        &[0, 0, 0, 0, 0, 0, 0, 1][..],
        &[0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1][..],
        &[0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1][..],
    ] {
        let mut payload = prefix.to_vec();
        payload.extend_from_slice(&negative_id);
        payload.extend_from_slice(&[0; 5]);
        assert_eq!(parse_play_packet(18, &payload), Err(PlayPacketError::InvalidRegistryId(-1)));
    }
    for length in 0..CONTAINER_CLICK.len() {
        assert!(
            matches!(parse_play_packet(18, &CONTAINER_CLICK[..length]), Err(PlayPacketError::Wire(_))),
            "truncation at {length}"
        );
    }
    let mut extra = CONTAINER_CLICK.to_vec();
    extra.push(0);
    assert_eq!(parse_play_packet(18, &extra), Err(PlayPacketError::TrailingBytes));
}

#[test]
fn use_item_on_decodes_26_3_block_hit() {
    assert_eq!(
        parse_play_packet(66, &USE_ITEM_ON).unwrap(),
        ServerboundPlay::UseItemOn {
            hand:             Hand::Off,
            position:         (-1, 63, 2),
            direction:        BlockFace::East,
            hit_offset:       (0.25, 0.5, 0.75),
            inside_block:     true,
            world_border_hit: false,
            sequence:         300,
        }
    );
}

#[test]
fn use_item_on_direction_wraps_positive_ids_and_preserves_border_hit() {
    let mut payload = USE_ITEM_ON.to_vec();
    payload.splice(9..14, [8]); // north, wrapped from 8
    payload[0] = 0;
    payload[22] = 0;
    payload[23] = 1;
    assert_eq!(
        parse_play_packet(66, &payload).unwrap(),
        ServerboundPlay::UseItemOn {
            hand:             Hand::Main,
            position:         (-1, 63, 2),
            direction:        BlockFace::North,
            hit_offset:       (0.25, 0.5, 0.75),
            inside_block:     false,
            world_border_hit: true,
            sequence:         300,
        }
    );
}

#[test]
fn use_item_on_treats_nonzero_boolean_bytes_as_true() {
    let mut payload = USE_ITEM_ON;
    payload[26] = 2;
    payload[27] = 0xff;
    assert_eq!(
        parse_play_packet(66, &payload).unwrap(),
        ServerboundPlay::UseItemOn {
            hand:             Hand::Off,
            position:         (-1, 63, 2),
            direction:        BlockFace::East,
            hit_offset:       (0.25, 0.5, 0.75),
            inside_block:     true,
            world_border_hit: true,
            sequence:         300,
        }
    );
}

#[test]
fn use_item_on_rejects_truncation_and_trailing_data() {
    for length in 0..USE_ITEM_ON.len() {
        assert!(
            matches!(parse_play_packet(66, &USE_ITEM_ON[..length]), Err(PlayPacketError::Wire(_))),
            "truncation at {length}"
        );
    }
    let mut extra = USE_ITEM_ON.to_vec();
    extra.push(0);
    assert_eq!(parse_play_packet(66, &extra), Err(PlayPacketError::TrailingBytes));
}

#[test]
fn movement_packets_decode_positions_rotations_and_both_flags() {
    let flags = MovementFlags {
        on_ground:            true,
        horizontal_collision: true,
    };

    let mut position = POSITION.to_vec();
    position.push(3);
    assert_eq!(
        parse_play_packet(30, &position).unwrap(),
        ServerboundPlay::Position {
            x: 1.5,
            y: -2.0,
            z: 3.5,
            flags,
        }
    );

    let mut position_and_look = POSITION.to_vec();
    position_and_look.extend_from_slice(&[0xc2, 0xb4, 0, 0, 0x42, 0x34, 0, 0, 3]);
    assert_eq!(
        parse_play_packet(31, &position_and_look).unwrap(),
        ServerboundPlay::PositionAndLook {
            x: 1.5,
            y: -2.0,
            z: 3.5,
            yaw: -90.0,
            pitch: 45.0,
            flags,
        }
    );

    assert_eq!(
        parse_play_packet(32, &[0xc2, 0xb4, 0, 0, 0x42, 0x34, 0, 0, 2]).unwrap(),
        ServerboundPlay::Look {
            yaw:   -90.0,
            pitch: 45.0,
            flags: MovementFlags {
                on_ground:            false,
                horizontal_collision: true,
            },
        }
    );
    assert_eq!(
        parse_play_packet(33, &[1]).unwrap(),
        ServerboundPlay::MovementStatus(MovementFlags {
            on_ground:            true,
            horizontal_collision: false,
        })
    );
}

#[test]
fn movement_packets_reject_truncated_or_extra_payloads() {
    assert!(matches!(parse_play_packet(30, &POSITION), Err(PlayPacketError::Wire(_))));
    assert!(matches!(parse_play_packet(33, &[1, 0]), Err(PlayPacketError::TrailingBytes)));
}

#[test]
fn teleport_confirmation_preserves_the_full_26_3_pose() {
    let mut payload = vec![0xac, 0x02];
    payload.extend_from_slice(&POSITION);
    payload.extend_from_slice(&[0xc2, 0xb4, 0, 0, 0x42, 0x34, 0, 0]);
    assert_eq!(
        parse_play_packet(0, &payload).unwrap(),
        ServerboundPlay::TeleportConfirm {
            id:    300,
            x:     1.5,
            y:     -2.0,
            z:     3.5,
            yaw:   -90.0,
            pitch: 45.0,
        }
    );
}

#[test]
fn attack_keep_alive_and_held_slot_use_their_26_3_wire_types() {
    assert_eq!(
        parse_play_packet(1, &[0xac, 0x02]).unwrap(),
        ServerboundPlay::Attack {
            target_entity_id: 300,
        }
    );
    assert_eq!(
        parse_play_packet(28, &[1, 2, 3, 4, 5, 6, 7, 8]).unwrap(),
        ServerboundPlay::KeepAlive(0x0102_0304_0506_0708)
    );
    assert_eq!(parse_play_packet(54, &[0xff, 0xfe]).unwrap(), ServerboundPlay::HeldSlot(-2));
}

#[test]
fn client_command_accepts_all_three_26_3_actions() {
    for (ordinal, action) in [
        (0, ClientCommand::PerformRespawn),
        (1, ClientCommand::RequestStats),
        (2, ClientCommand::RequestGameruleValues),
    ] {
        assert_eq!(parse_play_packet(12, &[ordinal]).unwrap(), ServerboundPlay::ClientCommand(action));
    }
}

#[test]
fn client_command_rejects_invalid_and_malformed_payloads() {
    assert_eq!(parse_play_packet(12, &[3]), Err(PlayPacketError::InvalidClientCommand(3)));
    assert_eq!(
        parse_play_packet(12, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
        Err(PlayPacketError::InvalidClientCommand(-1))
    );
    assert!(matches!(parse_play_packet(12, &[]), Err(PlayPacketError::Wire(_))));
    assert!(matches!(parse_play_packet(12, &[0x80]), Err(PlayPacketError::Wire(_))));
    assert!(matches!(parse_play_packet(12, &[0xff, 0xff, 0xff, 0xff, 0x10]), Err(PlayPacketError::Wire(_))));
    assert_eq!(parse_play_packet(12, &[0, 0]), Err(PlayPacketError::TrailingBytes));
}

#[test]
fn use_item_preserves_sequence_and_rotation_and_maps_hand_like_the_reference() {
    let payload = [1, 0xac, 0x02, 0xc2, 0xb4, 0, 0, 0x42, 0x34, 0, 0];
    assert_eq!(
        parse_play_packet(67, &payload).unwrap(),
        ServerboundPlay::UseItem {
            hand:     Hand::Off,
            sequence: 300,
            yaw:      -90.0,
            pitch:    45.0,
        }
    );
    let mut unknown_hand = payload;
    unknown_hand[0] = 2;
    assert_eq!(
        parse_play_packet(67, &unknown_hand).unwrap(),
        ServerboundPlay::UseItem {
            hand:     Hand::Main,
            sequence: 300,
            yaw:      -90.0,
            pitch:    45.0,
        }
    );
}

#[test]
fn player_action_decodes_each_ordinal_and_unsigned_direction() {
    let actions = [
        PlayerAction::StartDestroyBlock,
        PlayerAction::ChangeDestroyDirection,
        PlayerAction::AbortDestroyBlock,
        PlayerAction::StopDestroyBlock,
        PlayerAction::DropAllItems,
        PlayerAction::DropItem,
        PlayerAction::ReleaseUseItem,
        PlayerAction::SwapItemWithOffhand,
        PlayerAction::Stab,
    ];
    for (ordinal, action) in actions.into_iter().enumerate() {
        let payload = [
            ordinal as u8,
            0xff,
            0xff,
            0xff,
            0xc0,
            0,
            0,
            0x20,
            0x3f,
            0xff,
            0xac,
            0x02,
        ];
        assert_eq!(
            parse_play_packet(41, &payload).unwrap(),
            ServerboundPlay::PlayerAction {
                action,
                position: (-1, 63, 2),
                direction: BlockFace::South,
                sequence: 300,
            }
        );
    }
    assert_eq!(parse_play_packet(41, &[9]), Err(PlayPacketError::InvalidAction(9)));
}

#[test]
fn interaction_packets_reject_truncation_trailing_bytes_and_malformed_varints() {
    for (id, payload) in [
        (0, &[0][..]),
        (1, &[0x80][..]),
        (28, &[0; 7][..]),
        (41, &[0; 9][..]),
        (54, &[0][..]),
        (67, &[0, 0][..]),
    ] {
        assert!(matches!(parse_play_packet(id, payload), Err(PlayPacketError::Wire(_))), "packet {id}");
    }
    assert_eq!(parse_play_packet(1, &[0, 1]), Err(PlayPacketError::TrailingBytes));
    assert!(matches!(parse_play_packet(1, &[0xff, 0xff, 0xff, 0xff, 0x10]), Err(PlayPacketError::Wire(_))));
}
