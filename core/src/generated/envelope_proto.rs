// Automatically generated rust module for 'envelope.proto' file

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
pub struct Envelope {
    pub public_key: Vec<u8>,
    pub payload_type: Vec<u8>,
    pub payload: Vec<u8>,
    pub signature: Vec<u8>,
}

impl<'a> MessageRead<'a> for Envelope {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.public_key = r.read_bytes(bytes)?.to_owned(),
                Ok(18) => msg.payload_type = r.read_bytes(bytes)?.to_owned(),
                Ok(26) => msg.payload = r.read_bytes(bytes)?.to_owned(),
                Ok(42) => msg.signature = r.read_bytes(bytes)?.to_owned(),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Envelope {
    fn get_size(&self) -> usize {
        0
        + if self.public_key.is_empty() { 0 } else { 1 + sizeof_len((&self.public_key).len()) }
        + if self.payload_type.is_empty() { 0 } else { 1 + sizeof_len((&self.payload_type).len()) }
        + if self.payload.is_empty() { 0 } else { 1 + sizeof_len((&self.payload).len()) }
        + if self.signature.is_empty() { 0 } else { 1 + sizeof_len((&self.signature).len()) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if !self.public_key.is_empty() { w.write_with_tag(10, |w| w.write_bytes(&**&self.public_key))?; }
        if !self.payload_type.is_empty() { w.write_with_tag(18, |w| w.write_bytes(&**&self.payload_type))?; }
        if !self.payload.is_empty() { w.write_with_tag(26, |w| w.write_bytes(&**&self.payload))?; }
        if !self.signature.is_empty() { w.write_with_tag(42, |w| w.write_bytes(&**&self.signature))?; }
        Ok(())
    }
}

