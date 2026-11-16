use rustbukkit_protocol::v26_3::parse_configuration_packet;
use rustbukkit_protocol::{
    ChatVisibility,
    ClientInformation,
    ConfigurationInbound,
    MainHand,
    ParticleStatus,
};

const CLIENT_INFORMATION: [u8; 14] = [
    5, b'e', b'n', b'_', b'u', b's', // locale
    8,    // view distance
    0,    // full chat
    0,    // chat colors disabled
    0x7f, // skin customization flags
    0,    // left hand
    0,    // text filtering disabled
    0,    // server listing disabled
    0,    // all particles
];

#[test]
fn client_information_decodes_exact_typed_values() {
    let payload = [
        5, b'e', b'n', b'_', b'u', b's', 0xfe, 2, 0xff, 0x80, 1, 0, 0x80, 2,
    ];
    assert_eq!(
        parse_configuration_packet(0, &payload).unwrap(),
        ConfigurationInbound::ClientInformation(ClientInformation {
            locale:                "en_us".to_owned(),
            view_distance:         -2,
            chat_visibility:       ChatVisibility::Hidden,
            chat_colors:           true,
            skin_parts:            0x80,
            main_hand:             MainHand::Right,
            text_filtering:        false,
            allows_server_listing: true,
            particle_status:       ParticleStatus::Minimal,
        })
    );
}

#[test]
fn client_information_retains_each_setting() {
    let original = parse_configuration_packet(0, &CLIENT_INFORMATION).unwrap();
    for (offset, value) in [
        (1, b'f'),
        (6, 0xfe),
        (7, 1),
        (8, 1),
        (9, 0x80),
        (10, 1),
        (11, 1),
        (12, 1),
        (13, 1),
    ] {
        let mut changed = CLIENT_INFORMATION;
        changed[offset] = value;
        assert_ne!(
            original,
            parse_configuration_packet(0, &changed).unwrap(),
            "setting at offset {offset} was discarded"
        );
    }
}

#[test]
fn client_information_maps_out_of_range_enum_ids_like_the_26_3_server() {
    let original = parse_configuration_packet(0, &CLIENT_INFORMATION).unwrap();

    let mut system_chat = CLIENT_INFORMATION;
    system_chat[7] = 1;
    assert_ne!(original, parse_configuration_packet(0, &system_chat).unwrap());

    let mut wrapped_chat = CLIENT_INFORMATION;
    wrapped_chat[7] = 3;
    assert_eq!(original, parse_configuration_packet(0, &wrapped_chat).unwrap());

    let mut hidden_chat = CLIENT_INFORMATION.to_vec();
    hidden_chat.splice(7..8, [0xff, 0xff, 0xff, 0xff, 0x0f]);
    let mut hidden_chat_canonical = CLIENT_INFORMATION;
    hidden_chat_canonical[7] = 2;
    assert_eq!(
        parse_configuration_packet(0, &hidden_chat).unwrap(),
        parse_configuration_packet(0, &hidden_chat_canonical).unwrap()
    );

    let mut right_hand = CLIENT_INFORMATION;
    right_hand[10] = 1;
    assert_ne!(original, parse_configuration_packet(0, &right_hand).unwrap());
    let mut invalid_hand = CLIENT_INFORMATION;
    invalid_hand[10] = 2;
    assert_eq!(original, parse_configuration_packet(0, &invalid_hand).unwrap());
    let mut negative_hand = CLIENT_INFORMATION.to_vec();
    negative_hand.splice(10..11, [0xff, 0xff, 0xff, 0xff, 0x0f]);
    assert_eq!(original, parse_configuration_packet(0, &negative_hand).unwrap());

    let mut decreased_particles = CLIENT_INFORMATION;
    decreased_particles[13] = 1;
    assert_ne!(original, parse_configuration_packet(0, &decreased_particles).unwrap());
    let mut wrapped_particles = CLIENT_INFORMATION;
    wrapped_particles[13] = 4;
    assert_eq!(
        parse_configuration_packet(0, &decreased_particles).unwrap(),
        parse_configuration_packet(0, &wrapped_particles).unwrap()
    );
    let mut negative_particles = CLIENT_INFORMATION.to_vec();
    negative_particles.splice(13..14, [0xff, 0xff, 0xff, 0xff, 0x0f]);
    let mut minimal_particles = CLIENT_INFORMATION;
    minimal_particles[13] = 2;
    assert_eq!(
        parse_configuration_packet(0, &negative_particles).unwrap(),
        parse_configuration_packet(0, &minimal_particles).unwrap()
    );
}

#[test]
fn client_information_treats_every_nonzero_boolean_as_true() {
    for offset in [8, 11, 12] {
        let mut canonical = CLIENT_INFORMATION;
        canonical[offset] = 1;
        for value in [2, 0x80, 0xff] {
            let mut nonzero = CLIENT_INFORMATION;
            nonzero[offset] = value;
            assert_eq!(
                parse_configuration_packet(0, &canonical).unwrap(),
                parse_configuration_packet(0, &nonzero).unwrap(),
                "boolean at offset {offset} rejected {value}"
            );
        }
    }
}

#[test]
fn client_information_rejects_truncated_and_trailing_payloads() {
    for length in 0..CLIENT_INFORMATION.len() {
        assert!(
            parse_configuration_packet(0, &CLIENT_INFORMATION[..length]).is_err(),
            "accepted truncation at {length}"
        );
    }
    let mut trailing = CLIENT_INFORMATION.to_vec();
    trailing.push(0);
    assert!(parse_configuration_packet(0, &trailing).is_err());
}

#[test]
fn finish_configuration_and_code_of_conduct_accept_empty_payloads() {
    for (id, expected) in [
        (3, ConfigurationInbound::Finish),
        (9, ConfigurationInbound::AcceptCodeOfConduct),
    ] {
        assert_eq!(parse_configuration_packet(id, &[]).unwrap(), expected);
        assert!(parse_configuration_packet(id, &[0]).is_err(), "packet {id} accepted trailing data");
    }
}

#[test]
fn configuration_keepalive_and_pong_accept_fixed_width_big_endian_payloads() {
    for (id, payload, expected) in [
        (
            4,
            &0x0123_4567_89ab_cdef_i64.to_be_bytes()[..],
            ConfigurationInbound::KeepAlive(0x0123_4567_89ab_cdef),
        ),
        (5, &(-0x0123_4567_i32).to_be_bytes()[..], ConfigurationInbound::Pong(-0x0123_4567)),
    ] {
        assert_eq!(parse_configuration_packet(id, payload).unwrap(), expected);
        for length in 0..payload.len() {
            assert!(
                parse_configuration_packet(id, &payload[..length]).is_err(),
                "packet {id} accepted {length} bytes"
            );
        }
        let mut trailing = payload.to_vec();
        trailing.push(0);
        assert!(parse_configuration_packet(id, &trailing).is_err(), "packet {id} accepted trailing data");
    }
}
