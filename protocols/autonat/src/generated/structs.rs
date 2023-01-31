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
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Message {
    pub type_pb: Option<structs::mod_Message::MessageType>,
    pub dial: Option<structs::mod_Message::Dial>,
    pub dialResponse: Option<structs::mod_Message::DialResponse>,
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
        + self.type_pb.as_ref().map_or(0, |m| 1 + sizeof_varint(*(m) as u64))
        + self.dial.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
        + self.dialResponse.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.type_pb { w.write_with_tag(8, |w| w.write_enum(*s as i32))?; }
        if let Some(ref s) = self.dial { w.write_with_tag(18, |w| w.write_message(s))?; }
        if let Some(ref s) = self.dialResponse { w.write_with_tag(26, |w| w.write_message(s))?; }
        Ok(())
    }
}

pub mod mod_Message {

use super::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct PeerInfo {
    pub id: Option<Vec<u8>>,
    pub addrs: Vec<Vec<u8>>,
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
        + self.id.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.addrs.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.id { w.write_with_tag(10, |w| w.write_bytes(&**s))?; }
        for s in &self.addrs { w.write_with_tag(18, |w| w.write_bytes(&**s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Dial {
    pub peer: Option<structs::mod_Message::PeerInfo>,
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
        + self.peer.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.peer { w.write_with_tag(10, |w| w.write_message(s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct DialResponse {
    pub status: Option<structs::mod_Message::ResponseStatus>,
    pub statusText: Option<String>,
    pub addr: Option<Vec<u8>>,
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
        + self.status.as_ref().map_or(0, |m| 1 + sizeof_varint(*(m) as u64))
        + self.statusText.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.addr.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.status { w.write_with_tag(8, |w| w.write_enum(*s as i32))?; }
        if let Some(ref s) = self.statusText { w.write_with_tag(18, |w| w.write_string(&**s))?; }
        if let Some(ref s) = self.addr { w.write_with_tag(26, |w| w.write_bytes(&**s))?; }
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

