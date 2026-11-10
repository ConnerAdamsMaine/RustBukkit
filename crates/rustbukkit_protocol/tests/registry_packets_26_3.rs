use rustbukkit_encoding::MAX_PACKET_LENGTH;
use rustbukkit_protocol::v26_3::registry::{
    NbtTag,
    RegistryData,
    RegistryEntry,
    RegistryPacketError,
    TagEntry,
    TagRegistry,
    encode_registry_data,
    encode_update_tags,
};

#[test]
fn registry_data_preserves_entry_order_and_optional_network_nbt() {
    let registry = RegistryData {
        id:      "minecraft:test".into(),
        entries: vec![
            RegistryEntry {
                id:   "minecraft:a".into(),
                data: None,
            },
            RegistryEntry {
                id:   "minecraft:b".into(),
                data: Some(NbtTag::Compound(vec![("v".into(), NbtTag::Int(42))])),
            },
        ],
    };
    let mut expected = vec![14];
    expected.extend_from_slice(b"minecraft:test");
    expected.push(2);
    expected.push(11);
    expected.extend_from_slice(b"minecraft:a");
    expected.push(0);
    expected.push(11);
    expected.extend_from_slice(b"minecraft:b");
    expected.extend_from_slice(&[1, 10, 3, 0, 1, b'v', 0, 0, 0, 42, 0]);

    assert_eq!(encode_registry_data(&registry, 128).unwrap(), expected);
}

#[test]
fn update_tags_encodes_registry_and_tag_maps_with_varint_entry_ids() {
    let registries = vec![TagRegistry {
        id:   "minecraft:block".into(),
        tags: vec![TagEntry {
            id:      "minecraft:t".into(),
            entries: vec![0, 127, 128, 300],
        }],
    }];
    let mut expected = vec![1, 15];
    expected.extend_from_slice(b"minecraft:block");
    expected.extend_from_slice(&[1, 11]);
    expected.extend_from_slice(b"minecraft:t");
    expected.extend_from_slice(&[4, 0, 127, 128, 1, 172, 2]);

    assert_eq!(encode_update_tags(&registries, 128).unwrap(), expected);
    assert_eq!(encode_update_tags(&[], 1).unwrap(), [0]);
}

#[test]
fn registry_packets_canonicalize_default_namespaces_and_reject_duplicates() {
    let registry = RegistryData {
        id:      "test".into(),
        entries: vec![
            RegistryEntry {
                id:   "a".into(),
                data: None,
            },
            RegistryEntry {
                id:   "minecraft:a".into(),
                data: None,
            },
        ],
    };
    assert_eq!(encode_registry_data(&registry, 128), Err(RegistryPacketError::DuplicateIdentifier));

    let registries = vec![TagRegistry {
        id:   "block".into(),
        tags: vec![
            TagEntry {
                id:      "t".into(),
                entries: vec![],
            },
            TagEntry {
                id:      "minecraft:t".into(),
                entries: vec![],
            },
        ],
    }];
    assert_eq!(encode_update_tags(&registries, 128), Err(RegistryPacketError::DuplicateIdentifier));
}

#[test]
fn registry_packets_reject_invalid_identifiers_and_negative_tag_ids() {
    let registry = RegistryData {
        id:      "minecraft:Invalid".into(),
        entries: vec![],
    };
    assert_eq!(encode_registry_data(&registry, 128), Err(RegistryPacketError::InvalidIdentifier));
    let registries = vec![TagRegistry {
        id:   "block".into(),
        tags: vec![TagEntry {
            id:      "t".into(),
            entries: vec![-1],
        }],
    }];
    assert_eq!(encode_update_tags(&registries, 128), Err(RegistryPacketError::InvalidEntryId));
}

#[test]
fn registry_packets_enforce_payload_budget_including_optional_nbt() {
    let registry = RegistryData {
        id:      "test".into(),
        entries: vec![RegistryEntry {
            id:   "a".into(),
            data: Some(NbtTag::String("hello".into())),
        }],
    };
    let encoded = encode_registry_data(&registry, 128).unwrap();
    assert_eq!(encode_registry_data(&registry, encoded.len()).unwrap(), encoded);
    assert_eq!(encode_registry_data(&registry, encoded.len() - 1), Err(RegistryPacketError::TooLarge));
    assert_eq!(encode_update_tags(&[], 0), Err(RegistryPacketError::TooLarge));
    assert_eq!(encode_update_tags(&[], MAX_PACKET_LENGTH), Err(RegistryPacketError::InvalidBudget));
}
