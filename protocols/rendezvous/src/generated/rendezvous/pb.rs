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
pub struct Message {
    pub type_pb: Option<rendezvous::pb::mod_Message::MessageType>,
    pub register: Option<rendezvous::pb::mod_Message::Register>,
    pub registerResponse: Option<rendezvous::pb::mod_Message::RegisterResponse>,
    pub unregister: Option<rendezvous::pb::mod_Message::Unregister>,
    pub discover: Option<rendezvous::pb::mod_Message::Discover>,
    pub discoverResponse: Option<rendezvous::pb::mod_Message::DiscoverResponse>,
}

impl<'a> MessageRead<'a> for Message {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.type_pb = Some(r.read_enum(bytes)?),
                Ok(18) => msg.register = Some(r.read_message::<rendezvous::pb::mod_Message::Register>(bytes)?),
                Ok(26) => msg.registerResponse = Some(r.read_message::<rendezvous::pb::mod_Message::RegisterResponse>(bytes)?),
                Ok(34) => msg.unregister = Some(r.read_message::<rendezvous::pb::mod_Message::Unregister>(bytes)?),
                Ok(42) => msg.discover = Some(r.read_message::<rendezvous::pb::mod_Message::Discover>(bytes)?),
                Ok(50) => msg.discoverResponse = Some(r.read_message::<rendezvous::pb::mod_Message::DiscoverResponse>(bytes)?),
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
        + self.type_pb.as_ref().map_or(0, |m| 1 + sizeof_varint(*(m) as u64))
        + self.register.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
        + self.registerResponse.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
        + self.unregister.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
        + self.discover.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
        + self.discoverResponse.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.type_pb { w.write_with_tag(8, |w| w.write_enum(*s as i32))?; }
        if let Some(ref s) = self.register { w.write_with_tag(18, |w| w.write_message(s))?; }
        if let Some(ref s) = self.registerResponse { w.write_with_tag(26, |w| w.write_message(s))?; }
        if let Some(ref s) = self.unregister { w.write_with_tag(34, |w| w.write_message(s))?; }
        if let Some(ref s) = self.discover { w.write_with_tag(42, |w| w.write_message(s))?; }
        if let Some(ref s) = self.discoverResponse { w.write_with_tag(50, |w| w.write_message(s))?; }
        Ok(())
    }
}

pub mod mod_Message {

use super::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Register {
    pub ns: Option<String>,
    pub signedPeerRecord: Option<Vec<u8>>,
    pub ttl: Option<u64>,
}

impl<'a> MessageRead<'a> for Register {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.ns = Some(r.read_string(bytes)?.to_owned()),
                Ok(18) => msg.signedPeerRecord = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(24) => msg.ttl = Some(r.read_uint64(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Register {
    fn get_size(&self) -> usize {
        0
        + self.ns.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.signedPeerRecord.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.ttl.as_ref().map_or(0, |m| 1 + sizeof_varint(*(m) as u64))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.ns { w.write_with_tag(10, |w| w.write_string(&**s))?; }
        if let Some(ref s) = self.signedPeerRecord { w.write_with_tag(18, |w| w.write_bytes(&**s))?; }
        if let Some(ref s) = self.ttl { w.write_with_tag(24, |w| w.write_uint64(*s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct RegisterResponse {
    pub status: Option<rendezvous::pb::mod_Message::ResponseStatus>,
    pub statusText: Option<String>,
    pub ttl: Option<u64>,
}

impl<'a> MessageRead<'a> for RegisterResponse {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.status = Some(r.read_enum(bytes)?),
                Ok(18) => msg.statusText = Some(r.read_string(bytes)?.to_owned()),
                Ok(24) => msg.ttl = Some(r.read_uint64(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for RegisterResponse {
    fn get_size(&self) -> usize {
        0
        + self.status.as_ref().map_or(0, |m| 1 + sizeof_varint(*(m) as u64))
        + self.statusText.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.ttl.as_ref().map_or(0, |m| 1 + sizeof_varint(*(m) as u64))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.status { w.write_with_tag(8, |w| w.write_enum(*s as i32))?; }
        if let Some(ref s) = self.statusText { w.write_with_tag(18, |w| w.write_string(&**s))?; }
        if let Some(ref s) = self.ttl { w.write_with_tag(24, |w| w.write_uint64(*s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Unregister {
    pub ns: Option<String>,
    pub id: Option<Vec<u8>>,
}

impl<'a> MessageRead<'a> for Unregister {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.ns = Some(r.read_string(bytes)?.to_owned()),
                Ok(18) => msg.id = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Unregister {
    fn get_size(&self) -> usize {
        0
        + self.ns.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.id.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.ns { w.write_with_tag(10, |w| w.write_string(&**s))?; }
        if let Some(ref s) = self.id { w.write_with_tag(18, |w| w.write_bytes(&**s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Discover {
    pub ns: Option<String>,
    pub limit: Option<u64>,
    pub cookie: Option<Vec<u8>>,
}

impl<'a> MessageRead<'a> for Discover {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.ns = Some(r.read_string(bytes)?.to_owned()),
                Ok(16) => msg.limit = Some(r.read_uint64(bytes)?),
                Ok(26) => msg.cookie = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Discover {
    fn get_size(&self) -> usize {
        0
        + self.ns.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.limit.as_ref().map_or(0, |m| 1 + sizeof_varint(*(m) as u64))
        + self.cookie.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.ns { w.write_with_tag(10, |w| w.write_string(&**s))?; }
        if let Some(ref s) = self.limit { w.write_with_tag(16, |w| w.write_uint64(*s))?; }
        if let Some(ref s) = self.cookie { w.write_with_tag(26, |w| w.write_bytes(&**s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct DiscoverResponse {
    pub registrations: Vec<rendezvous::pb::mod_Message::Register>,
    pub cookie: Option<Vec<u8>>,
    pub status: Option<rendezvous::pb::mod_Message::ResponseStatus>,
    pub statusText: Option<String>,
}

impl<'a> MessageRead<'a> for DiscoverResponse {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.registrations.push(r.read_message::<rendezvous::pb::mod_Message::Register>(bytes)?),
                Ok(18) => msg.cookie = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(24) => msg.status = Some(r.read_enum(bytes)?),
                Ok(34) => msg.statusText = Some(r.read_string(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for DiscoverResponse {
    fn get_size(&self) -> usize {
        0
        + self.registrations.iter().map(|s| 1 + sizeof_len((s).get_size())).sum::<usize>()
        + self.cookie.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.status.as_ref().map_or(0, |m| 1 + sizeof_varint(*(m) as u64))
        + self.statusText.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        for s in &self.registrations { w.write_with_tag(10, |w| w.write_message(s))?; }
        if let Some(ref s) = self.cookie { w.write_with_tag(18, |w| w.write_bytes(&**s))?; }
        if let Some(ref s) = self.status { w.write_with_tag(24, |w| w.write_enum(*s as i32))?; }
        if let Some(ref s) = self.statusText { w.write_with_tag(34, |w| w.write_string(&**s))?; }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MessageType {
    REGISTER = 0,
    REGISTER_RESPONSE = 1,
    UNREGISTER = 2,
    DISCOVER = 3,
    DISCOVER_RESPONSE = 4,
}

impl Default for MessageType {
    fn default() -> Self {
        MessageType::REGISTER
    }
}

impl From<i32> for MessageType {
    fn from(i: i32) -> Self {
        match i {
            0 => MessageType::REGISTER,
            1 => MessageType::REGISTER_RESPONSE,
            2 => MessageType::UNREGISTER,
            3 => MessageType::DISCOVER,
            4 => MessageType::DISCOVER_RESPONSE,
            _ => Self::default(),
        }
    }
}

impl<'a> From<&'a str> for MessageType {
    fn from(s: &'a str) -> Self {
        match s {
            "REGISTER" => MessageType::REGISTER,
            "REGISTER_RESPONSE" => MessageType::REGISTER_RESPONSE,
            "UNREGISTER" => MessageType::UNREGISTER,
            "DISCOVER" => MessageType::DISCOVER,
            "DISCOVER_RESPONSE" => MessageType::DISCOVER_RESPONSE,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ResponseStatus {
    OK = 0,
    E_INVALID_NAMESPACE = 100,
    E_INVALID_SIGNED_PEER_RECORD = 101,
    E_INVALID_TTL = 102,
    E_INVALID_COOKIE = 103,
    E_NOT_AUTHORIZED = 200,
    E_INTERNAL_ERROR = 300,
    E_UNAVAILABLE = 400,
}

impl Default for ResponseStatus {
    fn default() -> Self {
        ResponseStatus::OK
    }
}

impl From<i32> for ResponseStatus {
    fn from(i: i32) -> Self {
        match i {
            0 => ResponseStatus::OK,
            100 => ResponseStatus::E_INVALID_NAMESPACE,
            101 => ResponseStatus::E_INVALID_SIGNED_PEER_RECORD,
            102 => ResponseStatus::E_INVALID_TTL,
            103 => ResponseStatus::E_INVALID_COOKIE,
            200 => ResponseStatus::E_NOT_AUTHORIZED,
            300 => ResponseStatus::E_INTERNAL_ERROR,
            400 => ResponseStatus::E_UNAVAILABLE,
            _ => Self::default(),
        }
    }
}

impl<'a> From<&'a str> for ResponseStatus {
    fn from(s: &'a str) -> Self {
        match s {
            "OK" => ResponseStatus::OK,
            "E_INVALID_NAMESPACE" => ResponseStatus::E_INVALID_NAMESPACE,
            "E_INVALID_SIGNED_PEER_RECORD" => ResponseStatus::E_INVALID_SIGNED_PEER_RECORD,
            "E_INVALID_TTL" => ResponseStatus::E_INVALID_TTL,
            "E_INVALID_COOKIE" => ResponseStatus::E_INVALID_COOKIE,
            "E_NOT_AUTHORIZED" => ResponseStatus::E_NOT_AUTHORIZED,
            "E_INTERNAL_ERROR" => ResponseStatus::E_INTERNAL_ERROR,
            "E_UNAVAILABLE" => ResponseStatus::E_UNAVAILABLE,
            _ => Self::default(),
        }
    }
}

}

