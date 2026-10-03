mod login;

mod protocol;

use bytes::BytesMut;
// use login::LoginHandler;
// use protocol::*;
use uuid::Uuid;

pub use crate::network::login::LoginHandler;
pub use crate::network::protocol::{
    DamageTypeCompound,
    DimensionCompound,
    NBTBuilder,
    PacketReader,
    PacketWriter,
    read_varint,
    write_varint,
};

pub trait ByteWritable {
    fn write_varint<N: Into<i32>>(&mut self, value: N);

    fn write_string<S: AsRef<str>>(&mut self, s: S);

    fn write_byte<N: Into<u8>>(&mut self, value: N);

    fn write_short<N: Into<i16>>(&mut self, value: N);

    fn write_int<N: Into<i32>>(&mut self, value: N);

    fn write_long<N: Into<i64>>(&mut self, value: N);

    fn write_float<N: Into<f32>>(&mut self, value: N);

    fn write_double<N: Into<f64>>(&mut self, value: N);

    fn write_bool<B: Into<bool>>(&mut self, value: B);

    // TODO: @check : check the constraints on how we want to do this -
    //  May want something like: AsRef + AsBytes or something else
    fn write_uuid<U: AsRef<Uuid>>(&mut self, uuid: U);

    fn write_bytes<A: AsRef<[u8]>>(&mut self, bytes: A);

    fn finish(self) -> BytesMut;
}
