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
#[derive(Debug, Default, PartialEq, Clone)]
pub struct RPC {
    pub subscriptions: Vec<gossipsub::pb::mod_RPC::SubOpts>,
    pub publish: Vec<gossipsub::pb::Message>,
    pub control: Option<gossipsub::pb::ControlMessage>,
}

impl<'a> MessageRead<'a> for RPC {
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

impl MessageWrite for RPC {
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

use super::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct SubOpts {
    pub subscribe: Option<bool>,
    pub topic_id: Option<String>,
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
pub struct Message {
    pub from: Option<Vec<u8>>,
    pub data: Option<Vec<u8>>,
    pub seqno: Option<Vec<u8>>,
    pub topic: String,
    pub signature: Option<Vec<u8>>,
    pub key: Option<Vec<u8>>,
}

impl<'a> MessageRead<'a> for Message {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.from = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(18) => msg.data = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(26) => msg.seqno = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(34) => msg.topic = r.read_string(bytes)?.to_owned(),
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
pub struct ControlMessage {
    pub ihave: Vec<gossipsub::pb::ControlIHave>,
    pub iwant: Vec<gossipsub::pb::ControlIWant>,
    pub graft: Vec<gossipsub::pb::ControlGraft>,
    pub prune: Vec<gossipsub::pb::ControlPrune>,
}

impl<'a> MessageRead<'a> for ControlMessage {
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

impl MessageWrite for ControlMessage {
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
pub struct ControlIHave {
    pub topic_id: Option<String>,
    pub message_ids: Vec<Vec<u8>>,
}

impl<'a> MessageRead<'a> for ControlIHave {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.topic_id = Some(r.read_string(bytes)?.to_owned()),
                Ok(18) => msg.message_ids.push(r.read_bytes(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for ControlIHave {
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
pub struct ControlIWant {
    pub message_ids: Vec<Vec<u8>>,
}

impl<'a> MessageRead<'a> for ControlIWant {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.message_ids.push(r.read_bytes(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for ControlIWant {
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
pub struct ControlGraft {
    pub topic_id: Option<String>,
}

impl<'a> MessageRead<'a> for ControlGraft {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.topic_id = Some(r.read_string(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for ControlGraft {
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
pub struct ControlPrune {
    pub topic_id: Option<String>,
    pub peers: Vec<gossipsub::pb::PeerInfo>,
    pub backoff: Option<u64>,
}

impl<'a> MessageRead<'a> for ControlPrune {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.topic_id = Some(r.read_string(bytes)?.to_owned()),
                Ok(18) => msg.peers.push(r.read_message::<gossipsub::pb::PeerInfo>(bytes)?),
                Ok(24) => msg.backoff = Some(r.read_uint64(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for ControlPrune {
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
pub struct PeerInfo {
    pub peer_id: Option<Vec<u8>>,
    pub signed_peer_record: Option<Vec<u8>>,
}

impl<'a> MessageRead<'a> for PeerInfo {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.peer_id = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(18) => msg.signed_peer_record = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for PeerInfo {
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
pub struct TopicDescriptor {
    pub name: Option<String>,
    pub auth: Option<gossipsub::pb::mod_TopicDescriptor::AuthOpts>,
    pub enc: Option<gossipsub::pb::mod_TopicDescriptor::EncOpts>,
}

impl<'a> MessageRead<'a> for TopicDescriptor {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.name = Some(r.read_string(bytes)?.to_owned()),
                Ok(18) => msg.auth = Some(r.read_message::<gossipsub::pb::mod_TopicDescriptor::AuthOpts>(bytes)?),
                Ok(26) => msg.enc = Some(r.read_message::<gossipsub::pb::mod_TopicDescriptor::EncOpts>(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for TopicDescriptor {
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

use super::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct AuthOpts {
    pub mode: Option<gossipsub::pb::mod_TopicDescriptor::mod_AuthOpts::AuthMode>,
    pub keys: Vec<Vec<u8>>,
}

impl<'a> MessageRead<'a> for AuthOpts {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.mode = Some(r.read_enum(bytes)?),
                Ok(18) => msg.keys.push(r.read_bytes(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for AuthOpts {
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
pub struct EncOpts {
    pub mode: Option<gossipsub::pb::mod_TopicDescriptor::mod_EncOpts::EncMode>,
    pub key_hashes: Vec<Vec<u8>>,
}

impl<'a> MessageRead<'a> for EncOpts {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.mode = Some(r.read_enum(bytes)?),
                Ok(18) => msg.key_hashes.push(r.read_bytes(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for EncOpts {
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

