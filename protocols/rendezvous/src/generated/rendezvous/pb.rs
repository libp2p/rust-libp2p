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
pub struct Message {
    pub type_pb: Option<rendezvous::pb::mod_Message::MessageType>,
    pub register: Option<rendezvous::pb::mod_Message::Register>,
    pub registerResponse: Option<rendezvous::pb::mod_Message::RegisterResponse>,
    pub unregister: Option<rendezvous::pb::mod_Message::Unregister>,
    pub discover: Option<rendezvous::pb::mod_Message::Discover>,
    pub discoverResponse: Option<rendezvous::pb::mod_Message::DiscoverResponse>,
}


impl Default for Message {
    fn default() -> Self {
        Self {
            type_pb: None,
            register: None,
            registerResponse: None,
            unregister: None,
            discover: None,
            discoverResponse: None,
        }
    }
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
        + if self.type_pb.is_some() { 1 + sizeof_varint((*(self.type_pb.as_ref().unwrap())) as u64) } else { 0 }
        + if self.register.is_some() { 1 + sizeof_len((self.register.as_ref().unwrap()).get_size()) } else { 0 }
        + if self.registerResponse.is_some() { 1 + sizeof_len((self.registerResponse.as_ref().unwrap()).get_size()) } else { 0 }
        + if self.unregister.is_some() { 1 + sizeof_len((self.unregister.as_ref().unwrap()).get_size()) } else { 0 }
        + if self.discover.is_some() { 1 + sizeof_len((self.discover.as_ref().unwrap()).get_size()) } else { 0 }
        + if self.discoverResponse.is_some() { 1 + sizeof_len((self.discoverResponse.as_ref().unwrap()).get_size()) } else { 0 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.type_pb.is_some() { w.write_with_tag(8, |w| w.write_enum(*(self.type_pb.as_ref().unwrap()) as i32))?; }
        if self.register.is_some() { w.write_with_tag(18, |w| w.write_message(self.register.as_ref().unwrap()))?; }
        if self.registerResponse.is_some() { w.write_with_tag(26, |w| w.write_message(self.registerResponse.as_ref().unwrap()))?; }
        if self.unregister.is_some() { w.write_with_tag(34, |w| w.write_message(self.unregister.as_ref().unwrap()))?; }
        if self.discover.is_some() { w.write_with_tag(42, |w| w.write_message(self.discover.as_ref().unwrap()))?; }
        if self.discoverResponse.is_some() { w.write_with_tag(50, |w| w.write_message(self.discoverResponse.as_ref().unwrap()))?; }
        Ok(())
    }
}

pub mod mod_Message {

use super::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, PartialEq, Clone)]
pub struct Register {
    pub ns: Option<String>,
    pub signedPeerRecord: Option<Vec<u8>>,
    pub ttl: Option<u64>,
}


impl Default for Register {
    fn default() -> Self {
        Self {
            ns: None,
            signedPeerRecord: None,
            ttl: None,
        }
    }
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
        + if self.ns.is_some() { 1 + sizeof_len((self.ns.as_ref().unwrap()).len()) } else { 0 }
        + if self.signedPeerRecord.is_some() { 1 + sizeof_len((self.signedPeerRecord.as_ref().unwrap()).len()) } else { 0 }
        + if self.ttl.is_some() { 1 + sizeof_varint((*(self.ttl.as_ref().unwrap())) as u64) } else { 0 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.ns.is_some() { w.write_with_tag(10, |w| w.write_string(&self.ns.as_ref().unwrap()))?; }
        if self.signedPeerRecord.is_some() { w.write_with_tag(18, |w| w.write_bytes(&self.signedPeerRecord.as_ref().unwrap()))?; }
        if self.ttl.is_some() { w.write_with_tag(24, |w| w.write_uint64(*(self.ttl.as_ref().unwrap())))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, PartialEq, Clone)]
pub struct RegisterResponse {
    pub status: Option<rendezvous::pb::mod_Message::ResponseStatus>,
    pub statusText: Option<String>,
    pub ttl: Option<u64>,
}


impl Default for RegisterResponse {
    fn default() -> Self {
        Self {
            status: None,
            statusText: None,
            ttl: None,
        }
    }
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
        + if self.status.is_some() { 1 + sizeof_varint((*(self.status.as_ref().unwrap())) as u64) } else { 0 }
        + if self.statusText.is_some() { 1 + sizeof_len((self.statusText.as_ref().unwrap()).len()) } else { 0 }
        + if self.ttl.is_some() { 1 + sizeof_varint((*(self.ttl.as_ref().unwrap())) as u64) } else { 0 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.status.is_some() { w.write_with_tag(8, |w| w.write_enum(*(self.status.as_ref().unwrap()) as i32))?; }
        if self.statusText.is_some() { w.write_with_tag(18, |w| w.write_string(&self.statusText.as_ref().unwrap()))?; }
        if self.ttl.is_some() { w.write_with_tag(24, |w| w.write_uint64(*(self.ttl.as_ref().unwrap())))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, PartialEq, Clone)]
pub struct Unregister {
    pub ns: Option<String>,
    pub id: Option<Vec<u8>>,
}


impl Default for Unregister {
    fn default() -> Self {
        Self {
            ns: None,
            id: None,
        }
    }
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
        + if self.ns.is_some() { 1 + sizeof_len((self.ns.as_ref().unwrap()).len()) } else { 0 }
        + if self.id.is_some() { 1 + sizeof_len((self.id.as_ref().unwrap()).len()) } else { 0 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.ns.is_some() { w.write_with_tag(10, |w| w.write_string(&self.ns.as_ref().unwrap()))?; }
        if self.id.is_some() { w.write_with_tag(18, |w| w.write_bytes(&self.id.as_ref().unwrap()))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, PartialEq, Clone)]
pub struct Discover {
    pub ns: Option<String>,
    pub limit: Option<u64>,
    pub cookie: Option<Vec<u8>>,
}


impl Default for Discover {
    fn default() -> Self {
        Self {
            ns: None,
            limit: None,
            cookie: None,
        }
    }
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
        + if self.ns.is_some() { 1 + sizeof_len((self.ns.as_ref().unwrap()).len()) } else { 0 }
        + if self.limit.is_some() { 1 + sizeof_varint((*(self.limit.as_ref().unwrap())) as u64) } else { 0 }
        + if self.cookie.is_some() { 1 + sizeof_len((self.cookie.as_ref().unwrap()).len()) } else { 0 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.ns.is_some() { w.write_with_tag(10, |w| w.write_string(&self.ns.as_ref().unwrap()))?; }
        if self.limit.is_some() { w.write_with_tag(16, |w| w.write_uint64(*(self.limit.as_ref().unwrap())))?; }
        if self.cookie.is_some() { w.write_with_tag(26, |w| w.write_bytes(&self.cookie.as_ref().unwrap()))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, PartialEq, Clone)]
pub struct DiscoverResponse {
    pub registrations: Vec<rendezvous::pb::mod_Message::Register>,
    pub cookie: Option<Vec<u8>>,
    pub status: Option<rendezvous::pb::mod_Message::ResponseStatus>,
    pub statusText: Option<String>,
}


impl Default for DiscoverResponse {
    fn default() -> Self {
        Self {
            registrations: Vec::new(),
            cookie: None,
            status: None,
            statusText: None,
        }
    }
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
        + if self.cookie.is_some() { 1 + sizeof_len((self.cookie.as_ref().unwrap()).len()) } else { 0 }
        + if self.status.is_some() { 1 + sizeof_varint((*(self.status.as_ref().unwrap())) as u64) } else { 0 }
        + if self.statusText.is_some() { 1 + sizeof_len((self.statusText.as_ref().unwrap()).len()) } else { 0 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        for s in &self.registrations { w.write_with_tag(10, |w| w.write_message(s))?; }
        if self.cookie.is_some() { w.write_with_tag(18, |w| w.write_bytes(&self.cookie.as_ref().unwrap()))?; }
        if self.status.is_some() { w.write_with_tag(24, |w| w.write_enum(*(self.status.as_ref().unwrap()) as i32))?; }
        if self.statusText.is_some() { w.write_with_tag(34, |w| w.write_string(&self.statusText.as_ref().unwrap()))?; }
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

