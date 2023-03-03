// Automatically generated rust module for 'message.proto' file

#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(unused_imports)]
#![allow(unknown_lints)]
#![allow(clippy::all)]
#![cfg_attr(rustfmt, rustfmt_skip)]


use quick_protobuf::{MessageInfo, MessageRead, MessageWrite, BytesReader, Writer, WriterBackend, Result};
use quick_protobuf::sizeofs::*;
use super::super::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct HolePunch {
    pub type_pb: holepunch::pb::mod_HolePunch::Type,
    pub ObsAddrs: Vec<Vec<u8>>,
}

impl<'a> MessageRead<'a> for HolePunch {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.type_pb = r.read_enum(bytes)?,
                Ok(18) => msg.ObsAddrs.push(r.read_bytes(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for HolePunch {
    fn get_size(&self) -> usize {
        0
        + 1 + sizeof_varint(*(&self.type_pb) as u64)
        + self.ObsAddrs.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        w.write_with_tag(8, |w| w.write_enum(*&self.type_pb as i32))?;
        for s in &self.ObsAddrs { w.write_with_tag(18, |w| w.write_bytes(&**s))?; }
        Ok(())
    }
}

pub mod mod_HolePunch {


#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Type {
    CONNECT = 100,
    SYNC = 300,
}

impl Default for Type {
    fn default() -> Self {
        Type::CONNECT
    }
}

impl From<i32> for Type {
    fn from(i: i32) -> Self {
        match i {
            100 => Type::CONNECT,
            300 => Type::SYNC,
            _ => Self::default(),
        }
    }
}

impl<'a> From<&'a str> for Type {
    fn from(s: &'a str) -> Self {
        match s {
            "CONNECT" => Type::CONNECT,
            "SYNC" => Type::SYNC,
            _ => Self::default(),
        }
    }
}

}

