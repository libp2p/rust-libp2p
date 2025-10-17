// Automatically generated rust module for 'turbine.proto' file

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
pub struct Shred {
    pub message_id: u64,
    pub index: u32,
    pub publisher: Vec<u8>,
    pub data: Vec<u8>,
    pub signature: Vec<u8>,
}

impl<'a> MessageRead<'a> for Shred {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.message_id = r.read_uint64(bytes)?,
                Ok(16) => msg.index = r.read_uint32(bytes)?,
                Ok(26) => msg.publisher = r.read_bytes(bytes)?.to_owned(),
                Ok(34) => msg.data = r.read_bytes(bytes)?.to_owned(),
                Ok(42) => msg.signature = r.read_bytes(bytes)?.to_owned(),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Shred {
    fn get_size(&self) -> usize {
        0
        + if self.message_id == 0u64 { 0 } else { 1 + sizeof_varint(*(&self.message_id) as u64) }
        + if self.index == 0u32 { 0 } else { 1 + sizeof_varint(*(&self.index) as u64) }
        + if self.publisher.is_empty() { 0 } else { 1 + sizeof_len((&self.publisher).len()) }
        + if self.data.is_empty() { 0 } else { 1 + sizeof_len((&self.data).len()) }
        + if self.signature.is_empty() { 0 } else { 1 + sizeof_len((&self.signature).len()) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.message_id != 0u64 { w.write_with_tag(8, |w| w.write_uint64(*&self.message_id))?; }
        if self.index != 0u32 { w.write_with_tag(16, |w| w.write_uint32(*&self.index))?; }
        if !self.publisher.is_empty() { w.write_with_tag(26, |w| w.write_bytes(&**&self.publisher))?; }
        if !self.data.is_empty() { w.write_with_tag(34, |w| w.write_bytes(&**&self.data))?; }
        if !self.signature.is_empty() { w.write_with_tag(42, |w| w.write_bytes(&**&self.signature))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct PropellerMessage {
    pub shred: Option<propeller::pb::Shred>,
}

impl<'a> MessageRead<'a> for PropellerMessage {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.shred = Some(r.read_message::<propeller::pb::Shred>(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for PropellerMessage {
    fn get_size(&self) -> usize {
        0
        + self.shred.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.shred { w.write_with_tag(10, |w| w.write_message(s))?; }
        Ok(())
    }
}

