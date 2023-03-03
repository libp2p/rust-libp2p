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
    pub public_key: Option<keys_proto::PublicKey>,
    pub payload_type: Vec<u8>,
    pub payload: Vec<u8>,
    pub signature: Vec<u8>,
}

impl<'a> MessageRead<'a> for Envelope {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.public_key = Some(r.read_message::<keys_proto::PublicKey>(bytes)?),
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
        + self.public_key.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
        + if self.payload_type.is_empty() { 0 } else { 1 + sizeof_len((&self.payload_type).len()) }
        + if self.payload.is_empty() { 0 } else { 1 + sizeof_len((&self.payload).len()) }
        + if self.signature.is_empty() { 0 } else { 1 + sizeof_len((&self.signature).len()) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.public_key { w.write_with_tag(10, |w| w.write_message(s))?; }
        if !self.payload_type.is_empty() { w.write_with_tag(18, |w| w.write_bytes(&**&self.payload_type))?; }
        if !self.payload.is_empty() { w.write_with_tag(26, |w| w.write_bytes(&**&self.payload))?; }
        if !self.signature.is_empty() { w.write_with_tag(42, |w| w.write_bytes(&**&self.signature))?; }
        Ok(())
    }
}

