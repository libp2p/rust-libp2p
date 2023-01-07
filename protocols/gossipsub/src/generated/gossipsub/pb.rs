// Automatically generated rust module for 'rpc.proto' file

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
pub struct RPC<'a> {
    pub subscriptions: Vec<gossipsub::pb::mod_RPC::SubOpts<'a>>,
    pub publish: Vec<gossipsub::pb::Message<'a>>,
    pub control: Option<gossipsub::pb::ControlMessage<'a>>,
}

impl<'a> MessageRead<'a> for RPC<'a> {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.subscriptions.push(r.read_message::<gossipsub::pb::mod_RPC::SubOpts>(bytes)?),
                Ok(18) => msg.publish.push(r.read_message::<gossipsub::pb::Message>(bytes)?),
                Ok(26) => msg.control = Some(r.read_message::<gossipsub::pb::ControlMessage>(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl<'a> MessageWrite for RPC<'a> {
    fn get_size(&self) -> usize {
        0
        + self.subscriptions.iter().map(|s| 1 + sizeof_len((s).get_size())).sum::<usize>()
        + self.publish.iter().map(|s| 1 + sizeof_len((s).get_size())).sum::<usize>()
        + self.control.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        for s in &self.subscriptions { w.write_with_tag(10, |w| w.write_message(s))?; }
        for s in &self.publish { w.write_with_tag(18, |w| w.write_message(s))?; }
        if let Some(ref s) = self.control { w.write_with_tag(26, |w| w.write_message(s))?; }
        Ok(())
    }
}

pub mod mod_RPC {

use std::borrow::Cow;
use super::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct SubOpts<'a> {
    pub subscribe: Option<bool>,
    pub topic_id: Option<Cow<'a, str>>,
}

impl<'a> MessageRead<'a> for SubOpts<'a> {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.subscribe = Some(r.read_bool(bytes)?),
                Ok(18) => msg.topic_id = Some(r.read_string(bytes).map(Cow::Borrowed)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl<'a> MessageWrite for SubOpts<'a> {
    fn get_size(&self) -> usize {
        0
        + self.subscribe.as_ref().map_or(0, |m| 1 + sizeof_varint(*(m) as u64))
        + self.topic_id.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.subscribe { w.write_with_tag(8, |w| w.write_bool(*s))?; }
        if let Some(ref s) = self.topic_id { w.write_with_tag(18, |w| w.write_string(&**s))?; }
        Ok(())
    }
}

}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Message<'a> {
    pub from: Option<Cow<'a, [u8]>>,
    pub data: Option<Cow<'a, [u8]>>,
    pub seqno: Option<Cow<'a, [u8]>>,
    pub topic: Cow<'a, str>,
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
                Ok(34) => msg.topic = r.read_string(bytes).map(Cow::Borrowed)?,
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
        + 1 + sizeof_len((&self.topic).len())
        + self.signature.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.key.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.from { w.write_with_tag(10, |w| w.write_bytes(&**s))?; }
        if let Some(ref s) = self.data { w.write_with_tag(18, |w| w.write_bytes(&**s))?; }
        if let Some(ref s) = self.seqno { w.write_with_tag(26, |w| w.write_bytes(&**s))?; }
        w.write_with_tag(34, |w| w.write_string(&**&self.topic))?;
        if let Some(ref s) = self.signature { w.write_with_tag(42, |w| w.write_bytes(&**s))?; }
        if let Some(ref s) = self.key { w.write_with_tag(50, |w| w.write_bytes(&**s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct ControlMessage<'a> {
    pub ihave: Vec<gossipsub::pb::ControlIHave<'a>>,
    pub iwant: Vec<gossipsub::pb::ControlIWant<'a>>,
    pub graft: Vec<gossipsub::pb::ControlGraft<'a>>,
    pub prune: Vec<gossipsub::pb::ControlPrune<'a>>,
}

impl<'a> MessageRead<'a> for ControlMessage<'a> {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.ihave.push(r.read_message::<gossipsub::pb::ControlIHave>(bytes)?),
                Ok(18) => msg.iwant.push(r.read_message::<gossipsub::pb::ControlIWant>(bytes)?),
                Ok(26) => msg.graft.push(r.read_message::<gossipsub::pb::ControlGraft>(bytes)?),
                Ok(34) => msg.prune.push(r.read_message::<gossipsub::pb::ControlPrune>(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl<'a> MessageWrite for ControlMessage<'a> {
    fn get_size(&self) -> usize {
        0
        + self.ihave.iter().map(|s| 1 + sizeof_len((s).get_size())).sum::<usize>()
        + self.iwant.iter().map(|s| 1 + sizeof_len((s).get_size())).sum::<usize>()
        + self.graft.iter().map(|s| 1 + sizeof_len((s).get_size())).sum::<usize>()
        + self.prune.iter().map(|s| 1 + sizeof_len((s).get_size())).sum::<usize>()
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        for s in &self.ihave { w.write_with_tag(10, |w| w.write_message(s))?; }
        for s in &self.iwant { w.write_with_tag(18, |w| w.write_message(s))?; }
        for s in &self.graft { w.write_with_tag(26, |w| w.write_message(s))?; }
        for s in &self.prune { w.write_with_tag(34, |w| w.write_message(s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct ControlIHave<'a> {
    pub topic_id: Option<Cow<'a, str>>,
    pub message_ids: Vec<Cow<'a, [u8]>>,
}

impl<'a> MessageRead<'a> for ControlIHave<'a> {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.topic_id = Some(r.read_string(bytes).map(Cow::Borrowed)?),
                Ok(18) => msg.message_ids.push(r.read_bytes(bytes).map(Cow::Borrowed)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl<'a> MessageWrite for ControlIHave<'a> {
    fn get_size(&self) -> usize {
        0
        + self.topic_id.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.message_ids.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.topic_id { w.write_with_tag(10, |w| w.write_string(&**s))?; }
        for s in &self.message_ids { w.write_with_tag(18, |w| w.write_bytes(&**s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct ControlIWant<'a> {
    pub message_ids: Vec<Cow<'a, [u8]>>,
}

impl<'a> MessageRead<'a> for ControlIWant<'a> {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.message_ids.push(r.read_bytes(bytes).map(Cow::Borrowed)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl<'a> MessageWrite for ControlIWant<'a> {
    fn get_size(&self) -> usize {
        0
        + self.message_ids.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        for s in &self.message_ids { w.write_with_tag(10, |w| w.write_bytes(&**s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct ControlGraft<'a> {
    pub topic_id: Option<Cow<'a, str>>,
}

impl<'a> MessageRead<'a> for ControlGraft<'a> {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.topic_id = Some(r.read_string(bytes).map(Cow::Borrowed)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl<'a> MessageWrite for ControlGraft<'a> {
    fn get_size(&self) -> usize {
        0
        + self.topic_id.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.topic_id { w.write_with_tag(10, |w| w.write_string(&**s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct ControlPrune<'a> {
    pub topic_id: Option<Cow<'a, str>>,
    pub peers: Vec<gossipsub::pb::PeerInfo<'a>>,
    pub backoff: Option<u64>,
}

impl<'a> MessageRead<'a> for ControlPrune<'a> {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.topic_id = Some(r.read_string(bytes).map(Cow::Borrowed)?),
                Ok(18) => msg.peers.push(r.read_message::<gossipsub::pb::PeerInfo>(bytes)?),
                Ok(24) => msg.backoff = Some(r.read_uint64(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl<'a> MessageWrite for ControlPrune<'a> {
    fn get_size(&self) -> usize {
        0
        + self.topic_id.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.peers.iter().map(|s| 1 + sizeof_len((s).get_size())).sum::<usize>()
        + self.backoff.as_ref().map_or(0, |m| 1 + sizeof_varint(*(m) as u64))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.topic_id { w.write_with_tag(10, |w| w.write_string(&**s))?; }
        for s in &self.peers { w.write_with_tag(18, |w| w.write_message(s))?; }
        if let Some(ref s) = self.backoff { w.write_with_tag(24, |w| w.write_uint64(*s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct PeerInfo<'a> {
    pub peer_id: Option<Cow<'a, [u8]>>,
    pub signed_peer_record: Option<Cow<'a, [u8]>>,
}

impl<'a> MessageRead<'a> for PeerInfo<'a> {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.peer_id = Some(r.read_bytes(bytes).map(Cow::Borrowed)?),
                Ok(18) => msg.signed_peer_record = Some(r.read_bytes(bytes).map(Cow::Borrowed)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl<'a> MessageWrite for PeerInfo<'a> {
    fn get_size(&self) -> usize {
        0
        + self.peer_id.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.signed_peer_record.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.peer_id { w.write_with_tag(10, |w| w.write_bytes(&**s))?; }
        if let Some(ref s) = self.signed_peer_record { w.write_with_tag(18, |w| w.write_bytes(&**s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct TopicDescriptor<'a> {
    pub name: Option<Cow<'a, str>>,
    pub auth: Option<gossipsub::pb::mod_TopicDescriptor::AuthOpts<'a>>,
    pub enc: Option<gossipsub::pb::mod_TopicDescriptor::EncOpts<'a>>,
}

impl<'a> MessageRead<'a> for TopicDescriptor<'a> {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.name = Some(r.read_string(bytes).map(Cow::Borrowed)?),
                Ok(18) => msg.auth = Some(r.read_message::<gossipsub::pb::mod_TopicDescriptor::AuthOpts>(bytes)?),
                Ok(26) => msg.enc = Some(r.read_message::<gossipsub::pb::mod_TopicDescriptor::EncOpts>(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl<'a> MessageWrite for TopicDescriptor<'a> {
    fn get_size(&self) -> usize {
        0
        + self.name.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.auth.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
        + self.enc.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.name { w.write_with_tag(10, |w| w.write_string(&**s))?; }
        if let Some(ref s) = self.auth { w.write_with_tag(18, |w| w.write_message(s))?; }
        if let Some(ref s) = self.enc { w.write_with_tag(26, |w| w.write_message(s))?; }
        Ok(())
    }
}

pub mod mod_TopicDescriptor {

use std::borrow::Cow;
use super::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct AuthOpts<'a> {
    pub mode: Option<gossipsub::pb::mod_TopicDescriptor::mod_AuthOpts::AuthMode>,
    pub keys: Vec<Cow<'a, [u8]>>,
}

impl<'a> MessageRead<'a> for AuthOpts<'a> {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.mode = Some(r.read_enum(bytes)?),
                Ok(18) => msg.keys.push(r.read_bytes(bytes).map(Cow::Borrowed)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl<'a> MessageWrite for AuthOpts<'a> {
    fn get_size(&self) -> usize {
        0
        + self.mode.as_ref().map_or(0, |m| 1 + sizeof_varint(*(m) as u64))
        + self.keys.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.mode { w.write_with_tag(8, |w| w.write_enum(*s as i32))?; }
        for s in &self.keys { w.write_with_tag(18, |w| w.write_bytes(&**s))?; }
        Ok(())
    }
}

pub mod mod_AuthOpts {


#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum AuthMode {
    NONE = 0,
    KEY = 1,
    WOT = 2,
}

impl Default for AuthMode {
    fn default() -> Self {
        AuthMode::NONE
    }
}

impl From<i32> for AuthMode {
    fn from(i: i32) -> Self {
        match i {
            0 => AuthMode::NONE,
            1 => AuthMode::KEY,
            2 => AuthMode::WOT,
            _ => Self::default(),
        }
    }
}

impl<'a> From<&'a str> for AuthMode {
    fn from(s: &'a str) -> Self {
        match s {
            "NONE" => AuthMode::NONE,
            "KEY" => AuthMode::KEY,
            "WOT" => AuthMode::WOT,
            _ => Self::default(),
        }
    }
}

}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct EncOpts<'a> {
    pub mode: Option<gossipsub::pb::mod_TopicDescriptor::mod_EncOpts::EncMode>,
    pub key_hashes: Vec<Cow<'a, [u8]>>,
}

impl<'a> MessageRead<'a> for EncOpts<'a> {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.mode = Some(r.read_enum(bytes)?),
                Ok(18) => msg.key_hashes.push(r.read_bytes(bytes).map(Cow::Borrowed)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl<'a> MessageWrite for EncOpts<'a> {
    fn get_size(&self) -> usize {
        0
        + self.mode.as_ref().map_or(0, |m| 1 + sizeof_varint(*(m) as u64))
        + self.key_hashes.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.mode { w.write_with_tag(8, |w| w.write_enum(*s as i32))?; }
        for s in &self.key_hashes { w.write_with_tag(18, |w| w.write_bytes(&**s))?; }
        Ok(())
    }
}

pub mod mod_EncOpts {


#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum EncMode {
    NONE = 0,
    SHAREDKEY = 1,
    WOT = 2,
}

impl Default for EncMode {
    fn default() -> Self {
        EncMode::NONE
    }
}

impl From<i32> for EncMode {
    fn from(i: i32) -> Self {
        match i {
            0 => EncMode::NONE,
            1 => EncMode::SHAREDKEY,
            2 => EncMode::WOT,
            _ => Self::default(),
        }
    }
}

impl<'a> From<&'a str> for EncMode {
    fn from(s: &'a str) -> Self {
        match s {
            "NONE" => EncMode::NONE,
            "SHAREDKEY" => EncMode::SHAREDKEY,
            "WOT" => EncMode::WOT,
            _ => Self::default(),
        }
    }
}

}

}

