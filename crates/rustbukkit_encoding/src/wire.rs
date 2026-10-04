use uuid::Uuid;

#[derive(Default)]
pub struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(capacity),
        }
    }

    pub fn varint(&mut self, value: i32) {
        let mut value = value as u32;
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            self.bytes.push(if value == 0 { byte } else { byte | 0x80 });
            if value == 0 {
                break;
            }
        }
    }

    pub fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub fn i8(&mut self, value: i8) {
        self.bytes.push(value as u8);
    }

    pub fn i16(&mut self, value: i16) {
        self.raw(&value.to_be_bytes());
    }

    pub fn i32(&mut self, value: i32) {
        self.raw(&value.to_be_bytes());
    }

    pub fn i64(&mut self, value: i64) {
        self.raw(&value.to_be_bytes());
    }

    pub fn f32(&mut self, value: f32) {
        self.raw(&value.to_be_bytes());
    }

    pub fn f64(&mut self, value: f64) {
        self.raw(&value.to_be_bytes());
    }

    pub fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    pub fn string(&mut self, value: &str) {
        self.varint(value.len() as i32);
        self.raw(value.as_bytes());
    }

    pub fn identifier(&mut self, value: &str) {
        self.string(value);
    }

    pub fn uuid(&mut self, value: Uuid) {
        self.raw(value.as_bytes());
    }

    pub fn position(&mut self, x: i32, y: i32, z: i32) {
        let packed = ((i64::from(x) & 0x3ff_ffff) << 38)
            | ((i64::from(z) & 0x3ff_ffff) << 12)
            | (i64::from(y) & 0xfff);
        self.i64(packed);
    }

    pub fn raw(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::Writer;

    #[test]
    fn known_wire_bytes_are_network_order() {
        let mut writer = Writer::new();
        writer.varint(300);
        writer.i16(0x1234);
        writer.i32(0x1234_5678);
        writer.i64(0x0102_0304_0506_0708);
        writer.f32(1.0);
        writer.f64(1.0);
        writer.uuid(Uuid::from_u128(0x0011_2233_4455_6677_8899_aabb_ccdd_eeff));
        writer.string("hi");
        assert_eq!(
            writer.finish(),
            [
                &[0xac, 0x02, 0x12, 0x34, 0x12, 0x34, 0x56, 0x78][..],
                &[1, 2, 3, 4, 5, 6, 7, 8],
                &[0x3f, 0x80, 0, 0],
                &[0x3f, 0xf0, 0, 0, 0, 0, 0, 0],
                &[
                    0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
                    0xff
                ],
                &[2, b'h', b'i'],
            ]
            .concat()
        );
    }
}
