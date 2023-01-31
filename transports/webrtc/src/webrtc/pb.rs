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
pub struct Message {
    pub flag: Option<webrtc::pb::mod_Message::Flag>,
    pub message: Option<Vec<u8>>,
}

impl<'a> MessageRead<'a> for Message {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.flag = Some(r.read_enum(bytes)?),
                Ok(18) => msg.message = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Message {
    fn get_size(&self) -> usize {
        0
        + self.flag.as_ref().map_or(0, |m| 1 + sizeof_varint(*(m) as u64))
        + self.message.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.flag { w.write_with_tag(8, |w| w.write_enum(*s as i32))?; }
        if let Some(ref s) = self.message { w.write_with_tag(18, |w| w.write_bytes(&**s))?; }
        Ok(())
    }
}

pub mod mod_Message {


#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Flag {
    FIN = 0,
    STOP_SENDING = 1,
    RESET = 2,
}

impl Default for Flag {
    fn default() -> Self {
        Flag::FIN
    }
}

impl From<i32> for Flag {
    fn from(i: i32) -> Self {
        match i {
            0 => Flag::FIN,
            1 => Flag::STOP_SENDING,
            2 => Flag::RESET,
            _ => Self::default(),
        }
    }
}

impl<'a> From<&'a str> for Flag {
    fn from(s: &'a str) -> Self {
        match s {
            "FIN" => Flag::FIN,
            "STOP_SENDING" => Flag::STOP_SENDING,
            "RESET" => Flag::RESET,
            _ => Self::default(),
        }
    }
}

}

