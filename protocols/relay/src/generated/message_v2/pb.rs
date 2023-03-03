// Automatically generated rust module for 'message.proto' file

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

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Status {
    OK = 100,
    RESERVATION_REFUSED = 200,
    RESOURCE_LIMIT_EXCEEDED = 201,
    PERMISSION_DENIED = 202,
    CONNECTION_FAILED = 203,
    NO_RESERVATION = 204,
    MALFORMED_MESSAGE = 400,
    UNEXPECTED_MESSAGE = 401,
}

impl Default for Status {
    fn default() -> Self {
        Status::OK
    }
}

impl From<i32> for Status {
    fn from(i: i32) -> Self {
        match i {
            100 => Status::OK,
            200 => Status::RESERVATION_REFUSED,
            201 => Status::RESOURCE_LIMIT_EXCEEDED,
            202 => Status::PERMISSION_DENIED,
            203 => Status::CONNECTION_FAILED,
            204 => Status::NO_RESERVATION,
            400 => Status::MALFORMED_MESSAGE,
            401 => Status::UNEXPECTED_MESSAGE,
            _ => Self::default(),
        }
    }
}

impl<'a> From<&'a str> for Status {
    fn from(s: &'a str) -> Self {
        match s {
            "OK" => Status::OK,
            "RESERVATION_REFUSED" => Status::RESERVATION_REFUSED,
            "RESOURCE_LIMIT_EXCEEDED" => Status::RESOURCE_LIMIT_EXCEEDED,
            "PERMISSION_DENIED" => Status::PERMISSION_DENIED,
            "CONNECTION_FAILED" => Status::CONNECTION_FAILED,
            "NO_RESERVATION" => Status::NO_RESERVATION,
            "MALFORMED_MESSAGE" => Status::MALFORMED_MESSAGE,
            "UNEXPECTED_MESSAGE" => Status::UNEXPECTED_MESSAGE,
            _ => Self::default(),
        }
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, PartialEq, Clone)]
pub struct HopMessage {
    pub type_pb: message_v2::pb::mod_HopMessage::Type,
    pub peer: Option<message_v2::pb::Peer>,
    pub reservation: Option<message_v2::pb::Reservation>,
    pub limit: Option<message_v2::pb::Limit>,
    pub status: Option<message_v2::pb::Status>,
}


impl Default for HopMessage {
    fn default() -> Self {
        Self {
            type_pb: message_v2::pb::mod_HopMessage::Type::RESERVE,
            peer: None,
            reservation: None,
            limit: None,
            status: None,
        }
    }
}

impl<'a> MessageRead<'a> for HopMessage {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.type_pb = r.read_enum(bytes)?,
                Ok(18) => msg.peer = Some(r.read_message::<message_v2::pb::Peer>(bytes)?),
                Ok(26) => msg.reservation = Some(r.read_message::<message_v2::pb::Reservation>(bytes)?),
                Ok(34) => msg.limit = Some(r.read_message::<message_v2::pb::Limit>(bytes)?),
                Ok(40) => msg.status = Some(r.read_enum(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for HopMessage {
    fn get_size(&self) -> usize {
        0
        + 1 + sizeof_varint((self.type_pb) as u64)
        + if self.peer.is_some() { 1 + sizeof_len((self.peer.as_ref().unwrap()).get_size()) } else { 0 }
        + if self.reservation.is_some() { 1 + sizeof_len((self.reservation.as_ref().unwrap()).get_size()) } else { 0 }
        + if self.limit.is_some() { 1 + sizeof_len((self.limit.as_ref().unwrap()).get_size()) } else { 0 }
        + if self.status.is_some() { 1 + sizeof_varint((*(self.status.as_ref().unwrap())) as u64) } else { 0 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        w.write_with_tag(8, |w| w.write_enum(self.type_pb as i32))?;
        if self.peer.is_some() { w.write_with_tag(18, |w| w.write_message(self.peer.as_ref().unwrap()))?; }
        if self.reservation.is_some() { w.write_with_tag(26, |w| w.write_message(self.reservation.as_ref().unwrap()))?; }
        if self.limit.is_some() { w.write_with_tag(34, |w| w.write_message(self.limit.as_ref().unwrap()))?; }
        if self.status.is_some() { w.write_with_tag(40, |w| w.write_enum(*(self.status.as_ref().unwrap()) as i32))?; }
        Ok(())
    }
}

pub mod mod_HopMessage {


#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Type {
    RESERVE = 0,
    CONNECT = 1,
    STATUS = 2,
}

impl Default for Type {
    fn default() -> Self {
        Type::RESERVE
    }
}

impl From<i32> for Type {
    fn from(i: i32) -> Self {
        match i {
            0 => Type::RESERVE,
            1 => Type::CONNECT,
            2 => Type::STATUS,
            _ => Self::default(),
        }
    }
}

impl<'a> From<&'a str> for Type {
    fn from(s: &'a str) -> Self {
        match s {
            "RESERVE" => Type::RESERVE,
            "CONNECT" => Type::CONNECT,
            "STATUS" => Type::STATUS,
            _ => Self::default(),
        }
    }
}

}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, PartialEq, Clone)]
pub struct StopMessage {
    pub type_pb: message_v2::pb::mod_StopMessage::Type,
    pub peer: Option<message_v2::pb::Peer>,
    pub limit: Option<message_v2::pb::Limit>,
    pub status: Option<message_v2::pb::Status>,
}


impl Default for StopMessage {
    fn default() -> Self {
        Self {
            type_pb: message_v2::pb::mod_StopMessage::Type::CONNECT,
            peer: None,
            limit: None,
            status: None,
        }
    }
}

impl<'a> MessageRead<'a> for StopMessage {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.type_pb = r.read_enum(bytes)?,
                Ok(18) => msg.peer = Some(r.read_message::<message_v2::pb::Peer>(bytes)?),
                Ok(26) => msg.limit = Some(r.read_message::<message_v2::pb::Limit>(bytes)?),
                Ok(32) => msg.status = Some(r.read_enum(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for StopMessage {
    fn get_size(&self) -> usize {
        0
        + 1 + sizeof_varint((self.type_pb) as u64)
        + if self.peer.is_some() { 1 + sizeof_len((self.peer.as_ref().unwrap()).get_size()) } else { 0 }
        + if self.limit.is_some() { 1 + sizeof_len((self.limit.as_ref().unwrap()).get_size()) } else { 0 }
        + if self.status.is_some() { 1 + sizeof_varint((*(self.status.as_ref().unwrap())) as u64) } else { 0 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        w.write_with_tag(8, |w| w.write_enum(self.type_pb as i32))?;
        if self.peer.is_some() { w.write_with_tag(18, |w| w.write_message(self.peer.as_ref().unwrap()))?; }
        if self.limit.is_some() { w.write_with_tag(26, |w| w.write_message(self.limit.as_ref().unwrap()))?; }
        if self.status.is_some() { w.write_with_tag(32, |w| w.write_enum(*(self.status.as_ref().unwrap()) as i32))?; }
        Ok(())
    }
}

pub mod mod_StopMessage {


#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Type {
    CONNECT = 0,
    STATUS = 1,
}

impl Default for Type {
    fn default() -> Self {
        Type::CONNECT
    }
}

impl From<i32> for Type {
    fn from(i: i32) -> Self {
        match i {
            0 => Type::CONNECT,
            1 => Type::STATUS,
            _ => Self::default(),
        }
    }
}

impl<'a> From<&'a str> for Type {
    fn from(s: &'a str) -> Self {
        match s {
            "CONNECT" => Type::CONNECT,
            "STATUS" => Type::STATUS,
            _ => Self::default(),
        }
    }
}

}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, PartialEq, Clone)]
pub struct Peer {
    pub id: Vec<u8>,
    pub addrs: Vec<Vec<u8>>,
}


impl Default for Peer {
    fn default() -> Self {
        Self {
            id: Vec::<u8>::new(),
            addrs: Vec::new(),
        }
    }
}

impl<'a> MessageRead<'a> for Peer {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.id = r.read_bytes(bytes)?.to_owned(),
                Ok(18) => msg.addrs.push(r.read_bytes(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Peer {
    fn get_size(&self) -> usize {
        0
        + 1 + sizeof_len((self.id).len())
        + self.addrs.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        w.write_with_tag(10, |w| w.write_bytes(&self.id))?;
        for s in &self.addrs { w.write_with_tag(18, |w| w.write_bytes(s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, PartialEq, Clone)]
pub struct Reservation {
    pub expire: u64,
    pub addrs: Vec<Vec<u8>>,
    pub voucher: Option<Vec<u8>>,
}


impl Default for Reservation {
    fn default() -> Self {
        Self {
            expire: 0u64,
            addrs: Vec::new(),
            voucher: None,
        }
    }
}

impl<'a> MessageRead<'a> for Reservation {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.expire = r.read_uint64(bytes)?,
                Ok(18) => msg.addrs.push(r.read_bytes(bytes)?.to_owned()),
                Ok(26) => msg.voucher = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Reservation {
    fn get_size(&self) -> usize {
        0
        + 1 + sizeof_varint((self.expire) as u64)
        + self.addrs.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
        + if self.voucher.is_some() { 1 + sizeof_len((self.voucher.as_ref().unwrap()).len()) } else { 0 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        w.write_with_tag(8, |w| w.write_uint64(self.expire))?;
        for s in &self.addrs { w.write_with_tag(18, |w| w.write_bytes(s))?; }
        if self.voucher.is_some() { w.write_with_tag(26, |w| w.write_bytes(&self.voucher.as_ref().unwrap()))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, PartialEq, Clone)]
pub struct Limit {
    pub duration: Option<u32>,
    pub data: Option<u64>,
}


impl Default for Limit {
    fn default() -> Self {
        Self {
            duration: None,
            data: None,
        }
    }
}

impl<'a> MessageRead<'a> for Limit {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.duration = Some(r.read_uint32(bytes)?),
                Ok(16) => msg.data = Some(r.read_uint64(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Limit {
    fn get_size(&self) -> usize {
        0
        + if self.duration.is_some() { 1 + sizeof_varint((*(self.duration.as_ref().unwrap())) as u64) } else { 0 }
        + if self.data.is_some() { 1 + sizeof_varint((*(self.data.as_ref().unwrap())) as u64) } else { 0 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.duration.is_some() { w.write_with_tag(8, |w| w.write_uint32(*(self.duration.as_ref().unwrap())))?; }
        if self.data.is_some() { w.write_with_tag(16, |w| w.write_uint64(*(self.data.as_ref().unwrap())))?; }
        Ok(())
    }
}

