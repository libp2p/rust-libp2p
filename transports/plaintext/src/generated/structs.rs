// Automatically generated rust module for 'structs.proto' file

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
#[derive(Debug, PartialEq, Clone)]
pub struct Exchange {
    pub id: Option<Vec<u8>>,
    pub pubkey: Option<Vec<u8>>,
}


impl Default for Exchange {
    fn default() -> Self {
        Self {
            id: None,
            pubkey: None,
        }
    }
}

impl<'a> MessageRead<'a> for Exchange {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.id = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(18) => msg.pubkey = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Exchange {
    fn get_size(&self) -> usize {
        0
        + if self.id.is_some() { 1 + sizeof_len((self.id.as_ref().unwrap()).len()) } else { 0 }
        + if self.pubkey.is_some() { 1 + sizeof_len((self.pubkey.as_ref().unwrap()).len()) } else { 0 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.id.is_some() { w.write_with_tag(10, |w| w.write_bytes(&self.id.as_ref().unwrap()))?; }
        if self.pubkey.is_some() { w.write_with_tag(18, |w| w.write_bytes(&self.pubkey.as_ref().unwrap()))?; }
        Ok(())
    }
}

