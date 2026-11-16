//! Byte-level golden tests for the 26.3 combat/health clientbound payloads.
//!
//! Expected bytes are hardcoded from the 26.3 layouts in
//! `HorizonX/horizonsx/crates/horizon-protocol/src/packets/play.rs`
//! (SetHealth, HurtAnimation, EntityEvent, SetEntityMotion) and the LpVec3
//! velocity codec ported from `horizon-protocol/src/buf.rs`. Packet IDs are
//! locked to `registry_data/26.3/packet_ids.json` by `protocol_ids_26_3.rs`.

use rustbukkit_encoding::Writer;
use rustbukkit_protocol::v26_3::clientbound::{
    encode_entity_event,
    encode_hurt_animation,
    encode_set_entity_motion,
    encode_set_health,
};
use rustbukkit_protocol::{ClientboundPacket, VersionRegistry};

#[test]
fn set_health_encodes_health_food_and_saturation_in_protocol_order() {
    // 20.0f32 = 41 A0 00 00 (BE), VarInt 20, 5.0f32 = 40 A0 00 00 (BE).
    let full = encode_set_health(20.0, 20, 5.0).unwrap();
    assert_eq!(full, [0x41, 0xA0, 0x00, 0x00, 0x14, 0x40, 0xA0, 0x00, 0x00]);

    // A dead player: health exactly zero, food/saturation unchanged.
    let dead = encode_set_health(0.0, 20, 5.0).unwrap();
    assert_eq!(dead, [0x00, 0x00, 0x00, 0x00, 0x14, 0x40, 0xA0, 0x00, 0x00]);

    // Damaged by one heart-half: 17.5f32 = 41 8C 00 00 (BE).
    let hurt = encode_set_health(17.5, 20, 5.0).unwrap();
    assert_eq!(hurt, [0x41, 0x8C, 0x00, 0x00, 0x14, 0x40, 0xA0, 0x00, 0x00]);
}

#[test]
fn set_health_rejects_non_finite_and_negative_health() {
    assert!(matches!(
        encode_set_health(f32::NAN, 20, 5.0),
        Err(rustbukkit_protocol::v26_3::clientbound::ClientboundError::InvalidHealth(_))
    ));
    assert!(matches!(
        encode_set_health(f32::INFINITY, 20, 5.0),
        Err(rustbukkit_protocol::v26_3::clientbound::ClientboundError::InvalidHealth(_))
    ));
    assert!(matches!(
        encode_set_health(-0.5, 20, 5.0),
        Err(rustbukkit_protocol::v26_3::clientbound::ClientboundError::InvalidHealth(_))
    ));
}

#[test]
fn hurt_animation_encodes_varint_entity_id_and_yaw_float() {
    // 74 = single-byte VarInt; 90.0f32 = 42 B4 00 00 (BE).
    let anim = encode_hurt_animation(74, 90.0);
    assert_eq!(anim, [0x4A, 0x42, 0xB4, 0x00, 0x00]);

    // Entity ids above 127 use multi-byte VarInts: 300 -> AC 02.
    let anim = encode_hurt_animation(300, 0.0);
    assert_eq!(anim, [0xAC, 0x02, 0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn entity_event_encodes_int_entity_id_and_event_byte() {
    // Vanilla hurt event 2: BE i32 entity id then the signed event byte.
    let hurt = encode_entity_event(74, 2);
    assert_eq!(hurt, [0x00, 0x00, 0x00, 0x4A, 0x02]);

    // Vanilla death event 3.
    let death = encode_entity_event(74, 3);
    assert_eq!(death, [0x00, 0x00, 0x00, 0x4A, 0x03]);
}

#[test]
fn set_entity_motion_encodes_varint_entity_id_and_lp_vec3_velocity() {
    // LpVec3 of [0.0, 0.4, 0.0]: scale 1, no partial-scale varint; the shared
    // 32-bit payload is 0x7FFF6661 written big-endian after two marker bytes.
    let push = encode_set_entity_motion(74, [0.0, 0.4, 0.0]);
    assert_eq!(push, [0x4A, 0xF9, 0xFF, 0x7F, 0xFF, 0x66, 0x61]);

    // A diagonal knockback within one block per tick.
    let push = encode_set_entity_motion(74, [0.4, 0.4, 0.0]);
    assert_eq!(push, [0x4A, 0xC1, 0xCC, 0x7F, 0xFF, 0x66, 0x62]);

    // Zero velocity is the single marker byte 0x00 (no axis payload).
    assert_eq!(encode_set_entity_motion(74, [0.0; 3]), [0x4A, 0x00]);

    // A large scale (40.5) requires the partial-scale VarInt suffix 0x0A
    // (scale 41 >> 2) after the fixed payload.
    let push = encode_set_entity_motion(74, [40.0, 0.0, -40.5]);
    assert_eq!(push, [0x4A, 0x75, 0xF3, 0x01, 0x90, 0xFF, 0xFF, 0x0A]);
}

/// Independently quantize with the same LpVec3 algorithm to confirm the
/// encoder is invertible within the codec's precision, mirroring the HorizonX
/// `lp_vec3_round_trips_within_precision` property test.
#[test]
fn lp_vec3_encoding_round_trips_within_precision() {
    for velocity in [
        [0.0, 0.4, 0.0],
        [0.4, 0.4, 0.0],
        [0.0, 0.0, 0.0],
        [12.0, 0.0, -40.5],
        [0.1, -0.2, 0.3],
        [0.0, -3.92, 0.0],
    ] {
        let payload = encode_set_entity_motion(1, velocity);
        let mut reader = Reader::new(&payload);
        assert_eq!(reader.varint().unwrap(), 1);
        let decoded = decode_lp_vec3(&mut reader);
        let scale = velocity
            .iter()
            .fold(0.0f64, |m, c| m.max(c.abs()))
            .ceil()
            .max(1.0);
        for component in 0..3 {
            assert!(
                (decoded[component] - velocity[component]).abs() <= scale / 32766.0 * 1.01,
                "{velocity:?} -> {decoded:?}"
            );
        }
        assert_eq!(reader.remaining(), 0, "no trailing bytes after a full LpVec3");
    }
}

#[test]
fn version_pack_routes_combat_ids_and_payloads() {
    let pack = VersionRegistry::select("26.3").unwrap();
    assert_eq!(pack.clientbound_id(ClientboundPacket::UpdateHealth), 106);
    assert_eq!(pack.clientbound_id(ClientboundPacket::HurtAnimation), 43);
    assert_eq!(pack.clientbound_id(ClientboundPacket::EntityEvent), 34);
    assert_eq!(pack.clientbound_id(ClientboundPacket::EntityVelocity), 103);

    assert_eq!(pack.update_health_payload(20.0, 20, 5.0), encode_set_health(20.0, 20, 5.0).unwrap());
    assert_eq!(pack.hurt_animation_payload(74, 90.0), encode_hurt_animation(74, 90.0));
    assert_eq!(pack.entity_event_payload(74, 2), encode_entity_event(74, 2));
    assert_eq!(
        pack.entity_velocity_payload(74, [0.0, 0.4, 0.0]),
        encode_set_entity_motion(74, [0.0, 0.4, 0.0])
    );

    // Respawn reuses the join payload's spawn context plus the keep byte.
    let respawn = pack.respawn_payload(3, 1_234, 63);
    let mut expected = Writer::new();
    expected.varint(0); // dimension type id: overworld
    expected.string("minecraft:overworld");
    expected.i64(1_234);
    expected.varint(0); // survival
    expected.varint(0); // no previous game mode
    expected.bool(false); // debug
    expected.bool(false); // flat
    expected.bool(false); // no last death location
    expected.varint(0); // portal cooldown
    expected.varint(63); // sea level
    expected.u8(3); // data-to-keep byte
    assert_eq!(respawn, expected.finish());
}

struct Reader<'a> {
    data:   &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn varint(&mut self) -> Result<i32, ()> {
        let mut value = 0u32;
        let mut shift = 0;
        loop {
            let byte = *self.data.get(self.offset).ok_or(())?;
            self.offset += 1;
            value |= u32::from(byte & 0x7F) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 28 {
                return Err(());
            }
        }
        Ok(value as i32)
    }

    fn u8(&mut self) -> Result<u8, ()> {
        let byte = *self.data.get(self.offset).ok_or(())?;
        self.offset += 1;
        Ok(byte)
    }

    fn u32(&mut self) -> Result<u32, ()> {
        if self.offset + 4 > self.data.len() {
            return Err(());
        }
        let slice = &self.data[self.offset..self.offset + 4];
        self.offset += 4;
        Ok(u32::from_be_bytes(slice.try_into().unwrap()))
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.offset
    }

    /// Decode the LpVec3 that follows, mirroring the vanilla
    /// `Vec3.LP_STREAM_CODEC` reader ported into `clientbound::lp_vec3`.
    fn lp_vec3(&mut self) -> Result<[f64; 3], ()> {
        let lowest = u64::from(self.u8()?);
        if lowest == 0 {
            return Ok([0.0; 3]);
        }
        let middle = u64::from(self.u8()?);
        let highest = u64::from(self.u32()?);
        let buffer = highest << 16 | middle << 8 | lowest;
        let mut scale = lowest & 3;
        if lowest & 4 == 4 {
            scale |= u64::try_from(self.varint()?).map_err(|_| ())? << 2;
        }
        let unpack = |v: u64| ((v & 32767) as f64).min(32766.0) * 2.0 / 32766.0 - 1.0;
        Ok([
            unpack(buffer >> 3) * scale as f64,
            unpack(buffer >> 18) * scale as f64,
            unpack(buffer >> 33) * scale as f64,
        ])
    }
}

fn decode_lp_vec3(reader: &mut Reader<'_>) -> [f64; 3] {
    reader.lp_vec3().unwrap()
}
