// Automatically generated rust module for 'rpc.proto' file

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
pub struct RPC {
    pub subscriptions: Vec<floodsub::pb::mod_RPC::SubOpts>,
    pub publish: Vec<floodsub::pb::Message>,
}


impl Default for RPC {
    fn default() -> Self {
        Self {
            subscriptions: Vec::new(),
            publish: Vec::new(),
        }
    }
}

impl<'a> MessageRead<'a> for RPC {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.subscriptions.push(r.read_message::<floodsub::pb::mod_RPC::SubOpts>(bytes)?),
                Ok(18) => msg.publish.push(r.read_message::<floodsub::pb::Message>(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for RPC {
    fn get_size(&self) -> usize {
        0
        + self.subscriptions.iter().map(|s| 1 + sizeof_len((s).get_size())).sum::<usize>()
        + self.publish.iter().map(|s| 1 + sizeof_len((s).get_size())).sum::<usize>()
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        for s in &self.subscriptions { w.write_with_tag(10, |w| w.write_message(s))?; }
        for s in &self.publish { w.write_with_tag(18, |w| w.write_message(s))?; }
        Ok(())
    }
}

pub mod mod_RPC {

use super::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, PartialEq, Clone)]
pub struct SubOpts {
    pub subscribe: Option<bool>,
    pub topic_id: Option<String>,
}


impl Default for SubOpts {
    fn default() -> Self {
        Self {
            subscribe: None,
            topic_id: None,
        }
    }
}

impl<'a> MessageRead<'a> for SubOpts {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.subscribe = Some(r.read_bool(bytes)?),
                Ok(18) => msg.topic_id = Some(r.read_string(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for SubOpts {
    fn get_size(&self) -> usize {
        0
        + if self.subscribe.is_some() { 1 + sizeof_varint((*(self.subscribe.as_ref().unwrap())) as u64) } else { 0 }
        + if self.topic_id.is_some() { 1 + sizeof_len((self.topic_id.as_ref().unwrap()).len()) } else { 0 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.subscribe.is_some() { w.write_with_tag(8, |w| w.write_bool(*(self.subscribe.as_ref().unwrap())))?; }
        if self.topic_id.is_some() { w.write_with_tag(18, |w| w.write_string(&self.topic_id.as_ref().unwrap()))?; }
        Ok(())
    }
}

}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, PartialEq, Clone)]
pub struct Message {
    pub from: Option<Vec<u8>>,
    pub data: Option<Vec<u8>>,
    pub seqno: Option<Vec<u8>>,
    pub topic_ids: Vec<String>,
}


impl Default for Message {
    fn default() -> Self {
        Self {
            from: None,
            data: None,
            seqno: None,
            topic_ids: Vec::new(),
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
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.from.is_some() { w.write_with_tag(10, |w| w.write_bytes(&self.from.as_ref().unwrap()))?; }
        if self.data.is_some() { w.write_with_tag(18, |w| w.write_bytes(&self.data.as_ref().unwrap()))?; }
        if self.seqno.is_some() { w.write_with_tag(26, |w| w.write_bytes(&self.seqno.as_ref().unwrap()))?; }
        for s in &self.topic_ids { w.write_with_tag(34, |w| w.write_string(s))?; }
        Ok(())
    }
}

