// Automatically generated rust module for 'peer_record.proto' file

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
pub struct PeerRecord {
    pub peer_id: Vec<u8>,
    pub seq: u64,
    pub addresses: Vec<peer_record_proto::mod_PeerRecord::AddressInfo>,
}

impl<'a> MessageRead<'a> for PeerRecord {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.peer_id = r.read_bytes(bytes)?.to_owned(),
                Ok(16) => msg.seq = r.read_uint64(bytes)?,
                Ok(26) => msg.addresses.push(r.read_message::<peer_record_proto::mod_PeerRecord::AddressInfo>(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for PeerRecord {
    fn get_size(&self) -> usize {
        0
        + if self.peer_id.is_empty() { 0 } else { 1 + sizeof_len((&self.peer_id).len()) }
        + if self.seq == 0u64 { 0 } else { 1 + sizeof_varint(*(&self.seq) as u64) }
        + self.addresses.iter().map(|s| 1 + sizeof_len((s).get_size())).sum::<usize>()
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if !self.peer_id.is_empty() { w.write_with_tag(10, |w| w.write_bytes(&**&self.peer_id))?; }
        if self.seq != 0u64 { w.write_with_tag(16, |w| w.write_uint64(*&self.seq))?; }
        for s in &self.addresses { w.write_with_tag(26, |w| w.write_message(s))?; }
        Ok(())
    }
}

pub mod mod_PeerRecord {

use super::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct AddressInfo {
    pub multiaddr: Vec<u8>,
}

impl<'a> MessageRead<'a> for AddressInfo {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.multiaddr = r.read_bytes(bytes)?.to_owned(),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for AddressInfo {
    fn get_size(&self) -> usize {
        0
        + if self.multiaddr.is_empty() { 0 } else { 1 + sizeof_len((&self.multiaddr).len()) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if !self.multiaddr.is_empty() { w.write_with_tag(10, |w| w.write_bytes(&**&self.multiaddr))?; }
        Ok(())
    }
}

}

