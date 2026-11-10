use rustbukkit_chunks::{BlockKind, BlockPos, ChunkPos, World};
use rustbukkit_decoding::Reader;
use rustbukkit_encoding::encode_frame;
use rustbukkit_protocol::v26_3::chunk::{
    BlockStateIds,
    ChunkError,
    FORGET_CHUNK_ID,
    LEVEL_CHUNK_WITH_LIGHT_ID,
    LIGHT_UPDATE_ID,
    LightData,
    SET_CHUNK_CACHE_CENTER_ID,
    encode_chunk_with_light,
    encode_forget_chunk,
    encode_light_update,
    encode_set_chunk_cache_center,
};

fn block_states() -> BlockStateIds {
    BlockStateIds {
        air:         0,
        stone:       11,
        water:       12,
        bedrock:     13,
        dirt:        14,
        grass_block: 15,
        sand:        16,
        cobblestone: 17,
    }
}

fn changed_chunk() -> rustbukkit_chunks::ChunkSnapshot {
    let world = World::new();
    for (x, z, block) in [
        (-16, 32, BlockKind::Stone),
        (-15, 32, BlockKind::Water),
        (-1, 47, BlockKind::Water),
    ] {
        world.set_block(BlockPos::new(x, 319, z).unwrap(), block);
    }
    world.snapshot_chunk(ChunkPos { x: -1, z: 2 })
}

fn empty_light() -> LightData<'static> {
    LightData {
        sky_mask:         0,
        block_mask:       0,
        empty_sky_mask:   0,
        empty_block_mask: 0,
        sky_arrays:       &[],
        block_arrays:     &[],
    }
}

#[test]
fn chunk_encodes_heightmaps_sections_palettes_and_light_in_26_3_order() {
    let snapshot = changed_chunk();
    let sky = [[0x5a; 2048], [0xa5; 2048]];
    let block = [[0x12; 2048]];
    let light = LightData {
        sky_mask:         (1 << 25) | 1,
        block_mask:       1 << 1,
        empty_sky_mask:   1 << 3,
        empty_block_mask: 1 << 4,
        sky_arrays:       &sky,
        block_arrays:     &block,
    };
    let payload = encode_chunk_with_light(-1, 2, &snapshot, block_states(), 7, &light).unwrap();
    let frame = encode_frame(LEVEL_CHUNK_WITH_LIGHT_ID, &payload).unwrap();
    let mut framed = Reader::new(&frame);
    let frame_length = framed.varint().unwrap() as usize;
    assert_eq!(frame_length, framed.remaining());
    assert_eq!(framed.varint().unwrap(), 46);
    assert_eq!(framed.raw(payload.len()).unwrap(), payload);
    assert_eq!(framed.remaining(), 0);

    let mut reader = Reader::new(&payload);
    assert_eq!(reader.i32().unwrap(), -1);
    assert_eq!(reader.i32().unwrap(), 2);
    assert_eq!(reader.varint().unwrap(), 3);
    for expected_type in [1, 4, 5] {
        assert_eq!(reader.varint().unwrap(), expected_type);
        assert_eq!(reader.varint().unwrap(), 37);
        let words: Vec<u64> = (0..37).map(|_| reader.i64().unwrap() as u64).collect();
        assert_eq!(words[0] & 0x1ff, 384);
        assert_eq!((words[0] >> 9) & 0x1ff, 384);
        assert_eq!((words[36] >> 27) & 0x1ff, 384);
    }

    let sections_len = reader.varint().unwrap() as usize;
    let sections = reader.raw(sections_len).unwrap();
    let mut sections = Reader::new(sections);
    for section_index in 0..24 {
        let non_air = sections.i16().unwrap();
        let fluid_count = sections.i16().unwrap();
        let bits = sections.u8().unwrap();
        let palette_len = if bits == 0 {
            1
        } else {
            sections.varint().unwrap() as usize
        };
        let palette: Vec<i32> = (0..palette_len).map(|_| sections.varint().unwrap()).collect();
        let words: Vec<u64> = if bits == 0 {
            Vec::new()
        } else {
            assert_eq!(bits, 4);
            (0..256).map(|_| sections.i64().unwrap() as u64).collect()
        };
        if section_index == 23 {
            assert_eq!(non_air, 3);
            assert_eq!(fluid_count, 2);
            assert_eq!(palette, [0, 11, 12]);
            assert_eq!(words[240], 0x21);
            assert_eq!(words[255], 0x2000_0000_0000_0000);
            assert!(words[..240].iter().all(|word| *word == 0));
            assert!(words[241..255].iter().all(|word| *word == 0));
        }
        assert_eq!(sections.u8().unwrap(), 0);
        assert_eq!(sections.varint().unwrap(), 7);
    }
    assert_eq!(sections.remaining(), 0);
    assert_eq!(reader.varint().unwrap(), 0);

    for mask in [((1 << 25) | 1), 1 << 1, 1 << 3, 1 << 4] {
        assert_eq!(reader.varint().unwrap(), 1);
        assert_eq!(reader.i64().unwrap() as u64, mask);
    }
    assert_eq!(reader.varint().unwrap(), 2);
    for layer in &sky {
        assert_eq!(reader.varint().unwrap(), 2048);
        assert_eq!(reader.raw(2048).unwrap(), layer);
    }
    assert_eq!(reader.varint().unwrap(), 1);
    assert_eq!(reader.varint().unwrap(), 2048);
    assert_eq!(reader.raw(2048).unwrap(), &block[0]);
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn chunk_rejects_negative_or_ambiguous_registry_ids() {
    let snapshot = changed_chunk();
    let mut invalid = block_states();
    invalid.water = -1;
    assert!(matches!(
        encode_chunk_with_light(-1, 2, &snapshot, invalid, 7, &empty_light()),
        Err(ChunkError::InvalidBlockStateId(-1))
    ));
    invalid = block_states();
    invalid.water = invalid.stone;
    assert!(matches!(
        encode_chunk_with_light(-1, 2, &snapshot, invalid, 7, &empty_light()),
        Err(ChunkError::DuplicateBlockStateId(11))
    ));
    assert!(matches!(
        encode_chunk_with_light(-1, 2, &snapshot, block_states(), -1, &empty_light()),
        Err(ChunkError::InvalidBiomeId(-1))
    ));
}

#[test]
fn light_data_rejects_overlapping_out_of_range_and_missing_sections() {
    let snapshot = changed_chunk();
    let overlap = LightData {
        sky_mask: 1,
        empty_sky_mask: 1,
        sky_arrays: &[[0; 2048]],
        ..empty_light()
    };
    let out_of_range = LightData {
        block_mask: 1 << 26,
        block_arrays: &[[0; 2048]],
        ..empty_light()
    };
    let missing = LightData {
        sky_mask: 1,
        ..empty_light()
    };
    for light in [&overlap, &out_of_range, &missing] {
        assert!(matches!(
            encode_chunk_with_light(-1, 2, &snapshot, block_states(), 7, light),
            Err(ChunkError::InvalidLightData)
        ));
        assert!(matches!(encode_light_update(-1, 2, light), Err(ChunkError::InvalidLightData)));
    }
}

#[test]
fn separate_light_and_chunk_tracking_packets_use_26_3_ids_and_coordinates() {
    assert_eq!(
        encode_frame(FORGET_CHUNK_ID, &encode_forget_chunk(-2, 3)).unwrap(),
        [0x09, 38, 0, 0, 0, 3, 0xff, 0xff, 0xff, 0xfe]
    );
    assert_eq!(
        encode_frame(SET_CHUNK_CACHE_CENTER_ID, &encode_set_chunk_cache_center(-1, 2)).unwrap(),
        [0x07, 96, 0xff, 0xff, 0xff, 0xff, 0x0f, 0x02]
    );
    assert_eq!(
        encode_frame(LIGHT_UPDATE_ID, &encode_light_update(300, -1, &empty_light()).unwrap()).unwrap(),
        [
            0x0e, 49, 0xac, 0x02, 0xff, 0xff, 0xff, 0xff, 0x0f, 0, 0, 0, 0, 0, 0
        ]
    );
}
