use rustbukkit_decoding::{Reader, WireError};
use rustbukkit_encoding::Writer;

use crate::version::{
    ChatVisibility,
    ClientInformation,
    ConfigurationError,
    ConfigurationInbound,
    LoginError,
    MainHand,
    ParticleStatus,
    validate_username,
};

pub mod chunk;
pub mod clientbound;
pub mod play;
pub mod protocol;
pub mod registry;

const MAX_UTF16_UNITS: usize = 32_767;
const MAX_KNOWN_PACKS: i32 = 64;

// 26.3 configuration packet IDs, from the generated `packets.json` report
// (`registry_data/26.3/packet_ids.json`); asserted by `tests/protocol_ids_26_3.rs`.
pub const CLIENTBOUND_CUSTOM_PAYLOAD_ID: i32 = 1;
pub const CLIENTBOUND_FINISH_CONFIGURATION_ID: i32 = 3;
pub const CLIENTBOUND_UPDATE_ENABLED_FEATURES_ID: i32 = 13;
pub const CLIENTBOUND_SELECT_KNOWN_PACKS_ID: i32 = 15;
pub const SERVERBOUND_CLIENT_INFORMATION_ID: i32 = 0;
pub const SERVERBOUND_CUSTOM_PAYLOAD_ID: i32 = 2;
pub const SERVERBOUND_FINISH_CONFIGURATION_ID: i32 = 3;
pub const SERVERBOUND_KEEP_ALIVE_ID: i32 = 4;
pub const SERVERBOUND_PONG_ID: i32 = 5;
pub const SERVERBOUND_SELECT_KNOWN_PACKS_ID: i32 = 7;
pub const SERVERBOUND_ACCEPT_CODE_OF_CONDUCT_ID: i32 = 9;

pub fn initial_configuration_packets() -> Vec<(i32, Vec<u8>)> {
    let mut brand = Writer::new();
    brand.string("minecraft:brand");
    brand.string("RustBukkit");

    let mut features = Writer::new();
    features.varint(1);
    features.string("minecraft:vanilla");

    let mut packs = Writer::new();
    packs.varint(1);
    packs.string("minecraft");
    packs.string("core");
    packs.string("26.3");

    vec![
        (CLIENTBOUND_CUSTOM_PAYLOAD_ID, brand.finish()),
        (CLIENTBOUND_UPDATE_ENABLED_FEATURES_ID, features.finish()),
        (CLIENTBOUND_SELECT_KNOWN_PACKS_ID, packs.finish()),
    ]
}

fn read_utf<'a>(reader: &mut Reader<'a>, max_units: usize) -> Result<&'a str, ConfigurationError> {
    let value = reader.string(max_units * 3)?;
    if value.encode_utf16().count() > max_units {
        return Err(ConfigurationError::Invalid("string exceeds UTF-16 length limit"));
    }
    Ok(value)
}

fn read_boolean(reader: &mut Reader<'_>) -> Result<bool, WireError> {
    Ok(reader.u8()? != 0)
}

fn parse_identifier(value: &str) -> Option<(&str, &str)> {
    let (namespace, path) = value.split_once(':').unwrap_or(("minecraft", value));
    let namespace = if namespace.is_empty() {
        "minecraft"
    } else {
        namespace
    };
    (namespace != ".."
        && namespace.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'.')
        })
        && path.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'.' | b'/')
        }))
    .then_some((namespace, path))
}

pub fn parse_configuration_packet(
    id: i32,
    payload: &[u8],
) -> Result<ConfigurationInbound, ConfigurationError> {
    let mut reader = Reader::new(payload);
    let packet = match id {
        SERVERBOUND_CLIENT_INFORMATION_ID => {
            ConfigurationInbound::ClientInformation(ClientInformation {
                locale:                read_utf(&mut reader, 16)?.to_owned(),
                view_distance:         reader.i8()?,
                chat_visibility:       match reader.varint()?.rem_euclid(3) {
                    0 => ChatVisibility::Full,
                    1 => ChatVisibility::System,
                    _ => ChatVisibility::Hidden,
                },
                chat_colors:           read_boolean(&mut reader)?,
                skin_parts:            reader.u8()?,
                main_hand:             match reader.varint()? {
                    1 => MainHand::Right,
                    _ => MainHand::Left,
                },
                text_filtering:        read_boolean(&mut reader)?,
                allows_server_listing: read_boolean(&mut reader)?,
                particle_status:       match reader.varint()?.rem_euclid(3) {
                    0 => ParticleStatus::All,
                    1 => ParticleStatus::Decreased,
                    _ => ParticleStatus::Minimal,
                },
            })
        }
        SERVERBOUND_CUSTOM_PAYLOAD_ID => {
            let channel = read_utf(&mut reader, MAX_UTF16_UNITS)?;
            let (namespace, path) = parse_identifier(channel)
                .ok_or(ConfigurationError::Invalid("invalid custom payload channel"))?;
            if namespace == "minecraft" && path == "brand" {
                read_utf(&mut reader, MAX_UTF16_UNITS)?;
            } else {
                if reader.remaining() > 32_767 {
                    return Err(ConfigurationError::Invalid("custom payload exceeds length limit"));
                }
                reader.raw(reader.remaining())?;
            }
            ConfigurationInbound::CustomPayload
        }
        SERVERBOUND_FINISH_CONFIGURATION_ID => ConfigurationInbound::Finish,
        SERVERBOUND_KEEP_ALIVE_ID => ConfigurationInbound::KeepAlive(reader.i64()?),
        SERVERBOUND_PONG_ID => ConfigurationInbound::Pong(reader.i32()?),
        SERVERBOUND_SELECT_KNOWN_PACKS_ID => {
            let count = reader.varint()?;
            if !(0..=MAX_KNOWN_PACKS).contains(&count) {
                return Err(ConfigurationError::Invalid("invalid known pack count"));
            }
            let mut has_core = count == 1;
            for _ in 0..count {
                let namespace = read_utf(&mut reader, MAX_UTF16_UNITS)?;
                let name = read_utf(&mut reader, MAX_UTF16_UNITS)?;
                let version = read_utf(&mut reader, MAX_UTF16_UNITS)?;
                has_core &= namespace == "minecraft" && name == "core" && version == "26.3";
            }
            ConfigurationInbound::KnownPacks { has_core }
        }
        SERVERBOUND_ACCEPT_CODE_OF_CONDUCT_ID => ConfigurationInbound::AcceptCodeOfConduct,
        _ => return Err(ConfigurationError::UnexpectedPacket(id)),
    };
    if reader.remaining() != 0 {
        return Err(ConfigurationError::Invalid("trailing bytes"));
    }
    Ok(packet)
}

pub fn parse_login_start(payload: &[u8]) -> Result<String, LoginError> {
    let mut reader = Reader::new(payload);
    let username = reader.string(16)?;
    validate_username(username)?;
    reader.uuid()?;
    if reader.remaining() != 0 {
        return Err(LoginError::InvalidPayload);
    }
    Ok(username.to_owned())
}

#[cfg(test)]
mod tests {
    use rustbukkit_encoding::Writer;

    use super::{ConfigurationInbound, parse_configuration_packet};

    #[test]
    fn configuration_channel_26_3_accepts_default_namespace_prefix_for_brand() {
        let mut payload = Writer::new();
        payload.string(":brand");
        payload.string("test client");
        assert!(matches!(
            parse_configuration_packet(2, &payload.finish()),
            Ok(ConfigurationInbound::CustomPayload)
        ));

        let mut malformed = Writer::new();
        malformed.string(":brand");
        assert!(parse_configuration_packet(2, &malformed.finish()).is_err());
    }

    #[test]
    fn configuration_channel_26_3_accepts_an_empty_path() {
        let mut payload = Writer::new();
        payload.string("minecraft:");
        assert!(matches!(
            parse_configuration_packet(2, &payload.finish()),
            Ok(ConfigurationInbound::CustomPayload)
        ));
    }

    #[test]
    fn configuration_channel_26_3_rejects_reserved_namespace() {
        let mut payload = Writer::new();
        payload.string("..:payload");
        assert!(parse_configuration_packet(2, &payload.finish()).is_err());
    }
}
