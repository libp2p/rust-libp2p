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
pub struct Identify {
    pub protocolVersion: Option<String>,
    pub agentVersion: Option<String>,
    pub publicKey: Option<Vec<u8>>,
    pub listenAddrs: Vec<Vec<u8>>,
    pub observedAddr: Option<Vec<u8>>,
    pub protocols: Vec<String>,
}


impl Default for Identify {
    fn default() -> Self {
        Self {
            protocolVersion: None,
            agentVersion: None,
            publicKey: None,
            listenAddrs: Vec::new(),
            observedAddr: None,
            protocols: Vec::new(),
        }
    }
}

impl<'a> MessageRead<'a> for Identify {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(42) => msg.protocolVersion = Some(r.read_string(bytes)?.to_owned()),
                Ok(50) => msg.agentVersion = Some(r.read_string(bytes)?.to_owned()),
                Ok(10) => msg.publicKey = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(18) => msg.listenAddrs.push(r.read_bytes(bytes)?.to_owned()),
                Ok(34) => msg.observedAddr = Some(r.read_bytes(bytes)?.to_owned()),
                Ok(26) => msg.protocols.push(r.read_string(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Identify {
    fn get_size(&self) -> usize {
        0
        + if self.protocolVersion.is_some() { 1 + sizeof_len((self.protocolVersion.as_ref().unwrap()).len()) } else { 0 }
        + if self.agentVersion.is_some() { 1 + sizeof_len((self.agentVersion.as_ref().unwrap()).len()) } else { 0 }
        + if self.publicKey.is_some() { 1 + sizeof_len((self.publicKey.as_ref().unwrap()).len()) } else { 0 }
        + self.listenAddrs.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
        + if self.observedAddr.is_some() { 1 + sizeof_len((self.observedAddr.as_ref().unwrap()).len()) } else { 0 }
        + self.protocols.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.protocolVersion.is_some() { w.write_with_tag(42, |w| w.write_string(&self.protocolVersion.as_ref().unwrap()))?; }
        if self.agentVersion.is_some() { w.write_with_tag(50, |w| w.write_string(&self.agentVersion.as_ref().unwrap()))?; }
        if self.publicKey.is_some() { w.write_with_tag(10, |w| w.write_bytes(&self.publicKey.as_ref().unwrap()))?; }
        for s in &self.listenAddrs { w.write_with_tag(18, |w| w.write_bytes(s))?; }
        if self.observedAddr.is_some() { w.write_with_tag(34, |w| w.write_bytes(&self.observedAddr.as_ref().unwrap()))?; }
        for s in &self.protocols { w.write_with_tag(26, |w| w.write_string(s))?; }
        Ok(())
    }
}

