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
use super::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct SignalingMessage {
    pub type_pb: signaling::mod_SignalingMessage::Type,
    pub data: String,
}

impl<'a> MessageRead<'a> for SignalingMessage {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.type_pb = r.read_enum(bytes)?,
                Ok(18) => msg.data = r.read_string(bytes)?.to_owned(),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for SignalingMessage {
    fn get_size(&self) -> usize {
        0
        + if self.type_pb == signaling::mod_SignalingMessage::Type::SDP_OFFER { 0 } else { 1 + sizeof_varint(*(&self.type_pb) as u64) }
        + if self.data == String::default() { 0 } else { 1 + sizeof_len((&self.data).len()) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.type_pb != signaling::mod_SignalingMessage::Type::SDP_OFFER { w.write_with_tag(8, |w| w.write_enum(*&self.type_pb as i32))?; }
        if self.data != String::default() { w.write_with_tag(18, |w| w.write_string(&**&self.data))?; }
        Ok(())
    }
}

pub(crate) mod mod_SignalingMessage {


#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Type {
    SDP_OFFER = 0,
    SDP_ANSWER = 1,
    ICE_CANDIDATE = 2,
}

impl Default for Type {
    fn default() -> Self {
        Type::SDP_OFFER
    }
}

impl From<i32> for Type {
    fn from(i: i32) -> Self {
        match i {
            0 => Type::SDP_OFFER,
            1 => Type::SDP_ANSWER,
            2 => Type::ICE_CANDIDATE,
            _ => Self::default(),
        }
    }
}

impl<'a> From<&'a str> for Type {
    fn from(s: &'a str) -> Self {
        match s {
            "SDP_OFFER" => Type::SDP_OFFER,
            "SDP_ANSWER" => Type::SDP_ANSWER,
            "ICE_CANDIDATE" => Type::ICE_CANDIDATE,
            _ => Self::default(),
        }
    }
}

}
