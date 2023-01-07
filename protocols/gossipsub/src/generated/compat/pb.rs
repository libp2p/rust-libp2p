// Automatically generated rust module for 'compat.proto' file

#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(unused_imports)]
#![allow(unknown_lints)]
#![allow(clippy::all)]
#![cfg_attr(rustfmt, rustfmt_skip)]


use std::borrow::Cow;
use quick_protobuf::{MessageInfo, MessageRead, MessageWrite, BytesReader, Writer, WriterBackend, Result};
use quick_protobuf::sizeofs::*;
use super::super::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Message<'a> {
    pub from: Option<Cow<'a, [u8]>>,
    pub data: Option<Cow<'a, [u8]>>,
    pub seqno: Option<Cow<'a, [u8]>>,
    pub topic_ids: Vec<Cow<'a, str>>,
    pub signature: Option<Cow<'a, [u8]>>,
    pub key: Option<Cow<'a, [u8]>>,
}

impl<'a> MessageRead<'a> for Message<'a> {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.from = Some(r.read_bytes(bytes).map(Cow::Borrowed)?),
                Ok(18) => msg.data = Some(r.read_bytes(bytes).map(Cow::Borrowed)?),
                Ok(26) => msg.seqno = Some(r.read_bytes(bytes).map(Cow::Borrowed)?),
                Ok(34) => msg.topic_ids.push(r.read_string(bytes).map(Cow::Borrowed)?),
                Ok(42) => msg.signature = Some(r.read_bytes(bytes).map(Cow::Borrowed)?),
                Ok(50) => msg.key = Some(r.read_bytes(bytes).map(Cow::Borrowed)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl<'a> MessageWrite for Message<'a> {
    fn get_size(&self) -> usize {
        0
        + self.from.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.data.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.seqno.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.topic_ids.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
        + self.signature.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.key.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.from { w.write_with_tag(10, |w| w.write_bytes(&**s))?; }
        if let Some(ref s) = self.data { w.write_with_tag(18, |w| w.write_bytes(&**s))?; }
        if let Some(ref s) = self.seqno { w.write_with_tag(26, |w| w.write_bytes(&**s))?; }
        for s in &self.topic_ids { w.write_with_tag(34, |w| w.write_string(&**s))?; }
        if let Some(ref s) = self.signature { w.write_with_tag(42, |w| w.write_bytes(&**s))?; }
        if let Some(ref s) = self.key { w.write_with_tag(50, |w| w.write_bytes(&**s))?; }
        Ok(())
    }
}

