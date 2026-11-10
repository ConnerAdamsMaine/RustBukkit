use rustbukkit_protocol::v26_3::registry::{NbtEncodeError, NbtTag, encode_network_nbt};

fn compound(fields: Vec<(&str, NbtTag)>) -> NbtTag {
    NbtTag::Compound(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
    )
}

#[test]
fn network_nbt_root_has_no_name() {
    let tag = compound(vec![("a", NbtTag::Int(42))]);
    assert_eq!(encode_network_nbt(&tag, 64).unwrap(), [10, 3, 0, 1, b'a', 0, 0, 0, 42, 0]);
}

#[test]
fn network_nbt_strings_and_names_use_java_modified_utf8() {
    let tag = compound(vec![("\0", NbtTag::String("A\0\u{20ac}\u{1f600}".into()))]);
    assert_eq!(
        encode_network_nbt(&tag, 64).unwrap(),
        [
            10, 8, 0, 2, 0xc0, 0x80, 0, 12, b'A', 0xc0, 0x80, 0xe2, 0x82, 0xac, 0xed, 0xa0, 0xbd, 0xed, 0xb8,
            0x80, 0,
        ]
    );
}

#[test]
fn network_nbt_string_length_is_limited_by_modified_utf8_bytes() {
    let accepted = NbtTag::String("a".repeat(65_535));
    let encoded = encode_network_nbt(&accepted, 70_000).unwrap();
    assert_eq!(&encoded[..3], &[8, 0xff, 0xff]);
    assert_eq!(encoded.len(), 65_538);

    let too_long = NbtTag::String(format!("{}\0", "a".repeat(65_534)));
    assert!(matches!(encode_network_nbt(&too_long, 70_000), Err(NbtEncodeError::StringTooLong)));
}

#[test]
fn network_nbt_scalar_and_array_payloads_are_big_endian() {
    let tag = NbtTag::List(vec![
        NbtTag::Short(0x1234),
        NbtTag::Long(0x0102_0304_0506_0708),
        NbtTag::Float(1.0),
        NbtTag::Double(1.0),
        NbtTag::ByteArray(vec![-1, 2]),
        NbtTag::IntArray(vec![1, -1]),
        NbtTag::LongArray(vec![2]),
    ]);
    assert_eq!(
        encode_network_nbt(&tag, 256).unwrap(),
        [
            9, 10, 0, 0, 0, 7, 2, 0, 0, 0x12, 0x34, 0, 4, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 0, 5, 0, 0, 0x3f,
            0x80, 0, 0, 0, 6, 0, 0, 0x3f, 0xf0, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 2, 0xff, 2, 0, 11, 0,
            0, 0, 0, 0, 2, 0, 0, 0, 1, 0xff, 0xff, 0xff, 0xff, 0, 12, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0,
            2, 0,
        ]
    );
}

#[test]
fn network_nbt_floating_point_nan_is_canonicalized_like_java() {
    let float = NbtTag::Float(f32::from_bits(0xffc0_0001));
    let double = NbtTag::Double(f64::from_bits(0xfff8_0000_0000_0001));
    assert_eq!(encode_network_nbt(&float, 16).unwrap(), [5, 0x7f, 0xc0, 0, 0]);
    assert_eq!(encode_network_nbt(&double, 16).unwrap(), [6, 0x7f, 0xf8, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn network_nbt_empty_and_homogeneous_lists_use_their_wire_element_type() {
    assert_eq!(encode_network_nbt(&NbtTag::List(vec![]), 64).unwrap(), [9, 0, 0, 0, 0, 0]);
    assert_eq!(
        encode_network_nbt(&NbtTag::List(vec![NbtTag::Int(1), NbtTag::Int(-2)]), 64).unwrap(),
        [9, 3, 0, 0, 0, 2, 0, 0, 0, 1, 0xff, 0xff, 0xff, 0xfe]
    );
}

#[test]
fn network_nbt_mixed_lists_wrap_non_compound_elements() {
    let tag = NbtTag::List(vec![
        NbtTag::Byte(7),
        compound(vec![("a", NbtTag::Byte(3))]),
        NbtTag::Int(2),
    ]);
    assert_eq!(
        encode_network_nbt(&tag, 64).unwrap(),
        [
            9, 10, 0, 0, 0, 3, 1, 0, 0, 7, 0, 1, 0, 1, b'a', 3, 0, 3, 0, 0, 0, 0, 0, 2, 0,
        ]
    );
}

#[test]
fn network_nbt_list_wraps_compounds_that_resemble_wrappers() {
    let tag = NbtTag::List(vec![compound(vec![("", NbtTag::Byte(5))])]);
    assert_eq!(encode_network_nbt(&tag, 64).unwrap(), [9, 10, 0, 0, 0, 1, 10, 0, 0, 1, 0, 0, 5, 0, 0]);
}

#[test]
fn network_nbt_rejects_duplicate_compound_names() {
    let tag = compound(vec![("a", NbtTag::Byte(1)), ("a", NbtTag::Int(2))]);
    assert!(matches!(encode_network_nbt(&tag, 64), Err(NbtEncodeError::DuplicateField)));
}

#[test]
fn network_nbt_enforces_the_requested_output_budget() {
    let tag = NbtTag::Int(42);
    assert_eq!(encode_network_nbt(&tag, 5).unwrap(), [3, 0, 0, 0, 42]);
    assert!(matches!(encode_network_nbt(&tag, 4), Err(NbtEncodeError::TooLarge)));
    assert!(matches!(
        encode_network_nbt(&tag, rustbukkit_encoding::MAX_PACKET_LENGTH + 1),
        Err(NbtEncodeError::InvalidBudget)
    ));
}

#[test]
fn network_nbt_limits_container_depth_to_512() {
    let mut tag = NbtTag::Byte(0);
    for _ in 0..512 {
        tag = NbtTag::List(vec![tag]);
    }
    assert!(encode_network_nbt(&tag, 4096).is_ok());
    let too_deep = NbtTag::List(vec![tag]);
    assert!(matches!(encode_network_nbt(&too_deep, 4096), Err(NbtEncodeError::DepthExceeded)));
}
