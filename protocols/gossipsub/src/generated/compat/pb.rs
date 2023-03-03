// Automatically generated rust module for 'compat.proto' file

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
#[derive(Debug, PartialEq, Clone)]
pub struct Message {
    pub from: Option<Vec<u8>>,
    pub data: Option<Vec<u8>>,
    pub seqno: Option<Vec<u8>>,
    pub topic_ids: Vec<String>,
    pub signature: Option<Vec<u8>>,
    pub key: Option<Vec<u8>>,
}


impl Default for Message {
    fn default() -> Self {
        Self {
            from: None,
            data: None,
            seqno: None,
            topic_ids: Vec::new(),
            signature: None,
            key: None,
        }
    }
}

impl<'a> MessageRead<'a> for Message {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.from = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(18) => msg.data = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(26) => msg.seqno = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(34) => msg.topic_ids.push(r.read_string(bytes)?.to_owned()),
                Ok(42) => msg.signature = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(50) => msg.key = Some(r.read_bytes(bytes)?.to_owned()),
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
        + if self.from.is_some() { 1 + sizeof_len((self.from.as_ref().unwrap()).len()) } else { 0 }
        + if self.data.is_some() { 1 + sizeof_len((self.data.as_ref().unwrap()).len()) } else { 0 }
        + if self.seqno.is_some() { 1 + sizeof_len((self.seqno.as_ref().unwrap()).len()) } else { 0 }
        + self.topic_ids.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
        + if self.signature.is_some() { 1 + sizeof_len((self.signature.as_ref().unwrap()).len()) } else { 0 }
        + if self.key.is_some() { 1 + sizeof_len((self.key.as_ref().unwrap()).len()) } else { 0 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.from.is_some() { w.write_with_tag(10, |w| w.write_bytes(&self.from.as_ref().unwrap()))?; }
        if self.data.is_some() { w.write_with_tag(18, |w| w.write_bytes(&self.data.as_ref().unwrap()))?; }
        if self.seqno.is_some() { w.write_with_tag(26, |w| w.write_bytes(&self.seqno.as_ref().unwrap()))?; }
        for s in &self.topic_ids { w.write_with_tag(34, |w| w.write_string(s))?; }
        if self.signature.is_some() { w.write_with_tag(42, |w| w.write_bytes(&self.signature.as_ref().unwrap()))?; }
        if self.key.is_some() { w.write_with_tag(50, |w| w.write_bytes(&self.key.as_ref().unwrap()))?; }
        Ok(())
    }
}

