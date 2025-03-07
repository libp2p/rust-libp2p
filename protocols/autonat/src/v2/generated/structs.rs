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

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum DialStatus {
    UNUSED = 0,
    E_DIAL_ERROR = 100,
    E_DIAL_BACK_ERROR = 101,
    OK = 200,
}

impl Default for DialStatus {
    fn default() -> Self {
        DialStatus::UNUSED
    }
}

impl From<i32> for DialStatus {
    fn from(i: i32) -> Self {
        match i {
            0 => DialStatus::UNUSED,
            100 => DialStatus::E_DIAL_ERROR,
            101 => DialStatus::E_DIAL_BACK_ERROR,
            200 => DialStatus::OK,
            _ => Self::default(),
        }
    }
}

impl<'a> From<&'a str> for DialStatus {
    fn from(s: &'a str) -> Self {
        match s {
            "UNUSED" => DialStatus::UNUSED,
            "E_DIAL_ERROR" => DialStatus::E_DIAL_ERROR,
            "E_DIAL_BACK_ERROR" => DialStatus::E_DIAL_BACK_ERROR,
            "OK" => DialStatus::OK,
            _ => Self::default(),
        }
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Message {
    pub msg: structs::mod_Message::OneOfmsg,
}

impl<'a> MessageRead<'a> for Message {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.msg = structs::mod_Message::OneOfmsg::dialRequest(r.read_message::<structs::DialRequest>(bytes)?),
                Ok(18) => msg.msg = structs::mod_Message::OneOfmsg::dialResponse(r.read_message::<structs::DialResponse>(bytes)?),
                Ok(26) => msg.msg = structs::mod_Message::OneOfmsg::dialDataRequest(r.read_message::<structs::DialDataRequest>(bytes)?),
                Ok(34) => msg.msg = structs::mod_Message::OneOfmsg::dialDataResponse(r.read_message::<structs::DialDataResponse>(bytes)?),
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
        + match self.msg {
            structs::mod_Message::OneOfmsg::dialRequest(ref m) => 1 + sizeof_len((m).get_size()),
            structs::mod_Message::OneOfmsg::dialResponse(ref m) => 1 + sizeof_len((m).get_size()),
            structs::mod_Message::OneOfmsg::dialDataRequest(ref m) => 1 + sizeof_len((m).get_size()),
            structs::mod_Message::OneOfmsg::dialDataResponse(ref m) => 1 + sizeof_len((m).get_size()),
            structs::mod_Message::OneOfmsg::None => 0,
    }    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        match self.msg {            structs::mod_Message::OneOfmsg::dialRequest(ref m) => { w.write_with_tag(10, |w| w.write_message(m))? },
            structs::mod_Message::OneOfmsg::dialResponse(ref m) => { w.write_with_tag(18, |w| w.write_message(m))? },
            structs::mod_Message::OneOfmsg::dialDataRequest(ref m) => { w.write_with_tag(26, |w| w.write_message(m))? },
            structs::mod_Message::OneOfmsg::dialDataResponse(ref m) => { w.write_with_tag(34, |w| w.write_message(m))? },
            structs::mod_Message::OneOfmsg::None => {},
    }        Ok(())
    }
}

pub mod mod_Message {

use super::*;

#[derive(Debug, PartialEq, Clone)]
pub enum OneOfmsg {
    dialRequest(structs::DialRequest),
    dialResponse(structs::DialResponse),
    dialDataRequest(structs::DialDataRequest),
    dialDataResponse(structs::DialDataResponse),
    None,
}

impl Default for OneOfmsg {
    fn default() -> Self {
        OneOfmsg::None
    }
}

}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct DialRequest {
    pub addrs: Vec<Vec<u8>>,
    pub nonce: u64,
}

impl<'a> MessageRead<'a> for DialRequest {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.addrs.push(r.read_bytes(bytes)?.to_owned()),
                Ok(17) => msg.nonce = r.read_fixed64(bytes)?,
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for DialRequest {
    fn get_size(&self) -> usize {
        0
        + self.addrs.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
        + if self.nonce == 0u64 { 0 } else { 1 + 8 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        for s in &self.addrs { w.write_with_tag(10, |w| w.write_bytes(&**s))?; }
        if self.nonce != 0u64 { w.write_with_tag(17, |w| w.write_fixed64(*&self.nonce))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct DialDataRequest {
    pub addrIdx: u32,
    pub numBytes: u64,
}

impl<'a> MessageRead<'a> for DialDataRequest {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.addrIdx = r.read_uint32(bytes)?,
                Ok(16) => msg.numBytes = r.read_uint64(bytes)?,
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for DialDataRequest {
    fn get_size(&self) -> usize {
        0
        + if self.addrIdx == 0u32 { 0 } else { 1 + sizeof_varint(*(&self.addrIdx) as u64) }
        + if self.numBytes == 0u64 { 0 } else { 1 + sizeof_varint(*(&self.numBytes) as u64) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.addrIdx != 0u32 { w.write_with_tag(8, |w| w.write_uint32(*&self.addrIdx))?; }
        if self.numBytes != 0u64 { w.write_with_tag(16, |w| w.write_uint64(*&self.numBytes))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct DialResponse {
    pub status: structs::mod_DialResponse::ResponseStatus,
    pub addrIdx: u32,
    pub dialStatus: structs::DialStatus,
}

impl<'a> MessageRead<'a> for DialResponse {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.status = r.read_enum(bytes)?,
                Ok(16) => msg.addrIdx = r.read_uint32(bytes)?,
                Ok(24) => msg.dialStatus = r.read_enum(bytes)?,
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
        + if self.status == structs::mod_DialResponse::ResponseStatus::E_INTERNAL_ERROR { 0 } else { 1 + sizeof_varint(*(&self.status) as u64) }
        + if self.addrIdx == 0u32 { 0 } else { 1 + sizeof_varint(*(&self.addrIdx) as u64) }
        + if self.dialStatus == structs::DialStatus::UNUSED { 0 } else { 1 + sizeof_varint(*(&self.dialStatus) as u64) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.status != structs::mod_DialResponse::ResponseStatus::E_INTERNAL_ERROR { w.write_with_tag(8, |w| w.write_enum(*&self.status as i32))?; }
        if self.addrIdx != 0u32 { w.write_with_tag(16, |w| w.write_uint32(*&self.addrIdx))?; }
        if self.dialStatus != structs::DialStatus::UNUSED { w.write_with_tag(24, |w| w.write_enum(*&self.dialStatus as i32))?; }
        Ok(())
    }
}

pub mod mod_DialResponse {


#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ResponseStatus {
    E_INTERNAL_ERROR = 0,
    E_REQUEST_REJECTED = 100,
    E_DIAL_REFUSED = 101,
    OK = 200,
}

impl Default for ResponseStatus {
    fn default() -> Self {
        ResponseStatus::E_INTERNAL_ERROR
    }
}

impl From<i32> for ResponseStatus {
    fn from(i: i32) -> Self {
        match i {
            0 => ResponseStatus::E_INTERNAL_ERROR,
            100 => ResponseStatus::E_REQUEST_REJECTED,
            101 => ResponseStatus::E_DIAL_REFUSED,
            200 => ResponseStatus::OK,
            _ => Self::default(),
        }
    }
}

impl<'a> From<&'a str> for ResponseStatus {
    fn from(s: &'a str) -> Self {
        match s {
            "E_INTERNAL_ERROR" => ResponseStatus::E_INTERNAL_ERROR,
            "E_REQUEST_REJECTED" => ResponseStatus::E_REQUEST_REJECTED,
            "E_DIAL_REFUSED" => ResponseStatus::E_DIAL_REFUSED,
            "OK" => ResponseStatus::OK,
            _ => Self::default(),
        }
    }
}

}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct DialDataResponse {
    pub data: Vec<u8>,
}

impl<'a> MessageRead<'a> for DialDataResponse {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.data = r.read_bytes(bytes)?.to_owned(),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for DialDataResponse {
    fn get_size(&self) -> usize {
        0
        + if self.data.is_empty() { 0 } else { 1 + sizeof_len((&self.data).len()) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if !self.data.is_empty() { w.write_with_tag(10, |w| w.write_bytes(&**&self.data))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct DialBack {
    pub nonce: u64,
}

impl<'a> MessageRead<'a> for DialBack {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(9) => msg.nonce = r.read_fixed64(bytes)?,
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for DialBack {
    fn get_size(&self) -> usize {
        0
        + if self.nonce == 0u64 { 0 } else { 1 + 8 }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.nonce != 0u64 { w.write_with_tag(9, |w| w.write_fixed64(*&self.nonce))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct DialBackResponse {
    pub status: structs::mod_DialBackResponse::DialBackStatus,
}

impl<'a> MessageRead<'a> for DialBackResponse {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.status = r.read_enum(bytes)?,
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for DialBackResponse {
    fn get_size(&self) -> usize {
        0
        + if self.status == structs::mod_DialBackResponse::DialBackStatus::OK { 0 } else { 1 + sizeof_varint(*(&self.status) as u64) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.status != structs::mod_DialBackResponse::DialBackStatus::OK { w.write_with_tag(8, |w| w.write_enum(*&self.status as i32))?; }
        Ok(())
    }
}

pub mod mod_DialBackResponse {


#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum DialBackStatus {
    OK = 0,
}

impl Default for DialBackStatus {
    fn default() -> Self {
        DialBackStatus::OK
    }
}

impl From<i32> for DialBackStatus {
    fn from(i: i32) -> Self {
        match i {
            0 => DialBackStatus::OK,
            _ => Self::default(),
        }
    }
}

impl<'a> From<&'a str> for DialBackStatus {
    fn from(s: &'a str) -> Self {
        match s {
            "OK" => DialBackStatus::OK,
            _ => Self::default(),
        }
    }
}

}

