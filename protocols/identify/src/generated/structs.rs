// Automatically generated rust module for 'structs.proto' file

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
use super::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Identify<'a> {
    pub protocolVersion: Option<Cow<'a, str>>,
    pub agentVersion: Option<Cow<'a, str>>,
    pub publicKey: Option<Cow<'a, [u8]>>,
    pub listenAddrs: Vec<Cow<'a, [u8]>>,
    pub observedAddr: Option<Cow<'a, [u8]>>,
    pub protocols: Vec<Cow<'a, str>>,
}

impl<'a> MessageRead<'a> for Identify<'a> {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(42) => msg.protocolVersion = Some(r.read_string(bytes).map(Cow::Borrowed)?),
                Ok(50) => msg.agentVersion = Some(r.read_string(bytes).map(Cow::Borrowed)?),
                Ok(10) => msg.publicKey = Some(r.read_bytes(bytes).map(Cow::Borrowed)?),
                Ok(18) => msg.listenAddrs.push(r.read_bytes(bytes).map(Cow::Borrowed)?),
                Ok(34) => msg.observedAddr = Some(r.read_bytes(bytes).map(Cow::Borrowed)?),
                Ok(26) => msg.protocols.push(r.read_string(bytes).map(Cow::Borrowed)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl<'a> MessageWrite for Identify<'a> {
    fn get_size(&self) -> usize {
        0
        + self.protocolVersion.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.agentVersion.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.publicKey.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.listenAddrs.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
        + self.observedAddr.as_ref().map_or(0, |m| 1 + sizeof_len((m).len()))
        + self.protocols.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.protocolVersion { w.write_with_tag(42, |w| w.write_string(&**s))?; }
        if let Some(ref s) = self.agentVersion { w.write_with_tag(50, |w| w.write_string(&**s))?; }
        if let Some(ref s) = self.publicKey { w.write_with_tag(10, |w| w.write_bytes(&**s))?; }
        for s in &self.listenAddrs { w.write_with_tag(18, |w| w.write_bytes(&**s))?; }
        if let Some(ref s) = self.observedAddr { w.write_with_tag(34, |w| w.write_bytes(&**s))?; }
        for s in &self.protocols { w.write_with_tag(26, |w| w.write_string(&**s))?; }
        Ok(())
    }
}

