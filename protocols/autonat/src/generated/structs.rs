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
pub struct Message {
    pub type_pb: Option<structs::mod_Message::MessageType>,
    pub dial: Option<structs::mod_Message::Dial>,
    pub dialResponse: Option<structs::mod_Message::DialResponse>,
}


impl Default for Message {
    fn default() -> Self {
        Self {
            type_pb: None,
            dial: None,
            dialResponse: None,
        }
    }
}

impl<'a> MessageRead<'a> for Message {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.type_pb = Some(r.read_enum(bytes)?),
                Ok(18) => msg.dial = Some(r.read_message::<structs::mod_Message::Dial>(bytes)?),
                Ok(26) => msg.dialResponse = Some(r.read_message::<structs::mod_Message::DialResponse>(bytes)?),
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
        + if self.dial.is_some() { 1 + sizeof_len((self.dial.as_ref().unwrap()).get_size()) } else { 0 }
        + if self.dialResponse.is_some() { 1 + sizeof_len((self.dialResponse.as_ref().unwrap()).get_size()) } else { 0 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.type_pb.is_some() { w.write_with_tag(8, |w| w.write_enum(*(self.type_pb.as_ref().unwrap()) as i32))?; }
        if self.dial.is_some() { w.write_with_tag(18, |w| w.write_message(self.dial.as_ref().unwrap()))?; }
        if self.dialResponse.is_some() { w.write_with_tag(26, |w| w.write_message(self.dialResponse.as_ref().unwrap()))?; }
        Ok(())
    }
}

pub mod mod_Message {

use super::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, PartialEq, Clone)]
pub struct PeerInfo {
    pub id: Option<Vec<u8>>,
    pub addrs: Vec<Vec<u8>>,
}


impl Default for PeerInfo {
    fn default() -> Self {
        Self {
            id: None,
            addrs: Vec::new(),
        }
    }
}

impl<'a> MessageRead<'a> for PeerInfo {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.id = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(18) => msg.addrs.push(r.read_bytes(bytes)?.to_owned()),
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
        + if self.id.is_some() { 1 + sizeof_len((self.id.as_ref().unwrap()).len()) } else { 0 }
        + self.addrs.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.id.is_some() { w.write_with_tag(10, |w| w.write_bytes(&self.id.as_ref().unwrap()))?; }
        for s in &self.addrs { w.write_with_tag(18, |w| w.write_bytes(s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, PartialEq, Clone)]
pub struct Dial {
    pub peer: Option<structs::mod_Message::PeerInfo>,
}


impl Default for Dial {
    fn default() -> Self {
        Self {
            peer: None,
        }
    }
}

impl<'a> MessageRead<'a> for Dial {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.peer = Some(r.read_message::<structs::mod_Message::PeerInfo>(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Dial {
    fn get_size(&self) -> usize {
        0
        + if self.peer.is_some() { 1 + sizeof_len((self.peer.as_ref().unwrap()).get_size()) } else { 0 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.peer.is_some() { w.write_with_tag(10, |w| w.write_message(self.peer.as_ref().unwrap()))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, PartialEq, Clone)]
pub struct DialResponse {
    pub status: Option<structs::mod_Message::ResponseStatus>,
    pub statusText: Option<String>,
    pub addr: Option<Vec<u8>>,
}


impl Default for DialResponse {
    fn default() -> Self {
        Self {
            status: None,
            statusText: None,
            addr: None,
        }
    }
}

impl<'a> MessageRead<'a> for DialResponse {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.status = Some(r.read_enum(bytes)?),
                Ok(18) => msg.statusText = Some(r.read_string(bytes)?.to_owned()),
                Ok(26) => msg.addr = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for DialResponse {
    fn get_size(&self) -> usize {
        0
        + if self.status.is_some() { 1 + sizeof_varint((*(self.status.as_ref().unwrap())) as u64) } else { 0 }
        + if self.statusText.is_some() { 1 + sizeof_len((self.statusText.as_ref().unwrap()).len()) } else { 0 }
        + if self.addr.is_some() { 1 + sizeof_len((self.addr.as_ref().unwrap()).len()) } else { 0 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.status.is_some() { w.write_with_tag(8, |w| w.write_enum(*(self.status.as_ref().unwrap()) as i32))?; }
        if self.statusText.is_some() { w.write_with_tag(18, |w| w.write_string(&self.statusText.as_ref().unwrap()))?; }
        if self.addr.is_some() { w.write_with_tag(26, |w| w.write_bytes(&self.addr.as_ref().unwrap()))?; }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MessageType {
    DIAL = 0,
    DIAL_RESPONSE = 1,
}

impl Default for MessageType {
    fn default() -> Self {
        MessageType::DIAL
    }
}

impl From<i32> for MessageType {
    fn from(i: i32) -> Self {
        match i {
            0 => MessageType::DIAL,
            1 => MessageType::DIAL_RESPONSE,
            _ => Self::default(),
        }
    }
}

impl<'a> From<&'a str> for MessageType {
    fn from(s: &'a str) -> Self {
        match s {
            "DIAL" => MessageType::DIAL,
            "DIAL_RESPONSE" => MessageType::DIAL_RESPONSE,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ResponseStatus {
    OK = 0,
    E_DIAL_ERROR = 100,
    E_DIAL_REFUSED = 101,
    E_BAD_REQUEST = 200,
    E_INTERNAL_ERROR = 300,
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
            100 => ResponseStatus::E_DIAL_ERROR,
            101 => ResponseStatus::E_DIAL_REFUSED,
            200 => ResponseStatus::E_BAD_REQUEST,
            300 => ResponseStatus::E_INTERNAL_ERROR,
            _ => Self::default(),
        }
    }
}

impl<'a> From<&'a str> for ResponseStatus {
    fn from(s: &'a str) -> Self {
        match s {
            "OK" => ResponseStatus::OK,
            "E_DIAL_ERROR" => ResponseStatus::E_DIAL_ERROR,
            "E_DIAL_REFUSED" => ResponseStatus::E_DIAL_REFUSED,
            "E_BAD_REQUEST" => ResponseStatus::E_BAD_REQUEST,
            "E_INTERNAL_ERROR" => ResponseStatus::E_INTERNAL_ERROR,
            _ => Self::default(),
        }
    }
}

}

