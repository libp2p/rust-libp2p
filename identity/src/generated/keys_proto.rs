// Automatically generated rust module for 'keys.proto' file

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
pub enum KeyType {
    RSA = 0,
    Ed25519 = 1,
    Secp256k1 = 2,
    ECDSA = 3,
}

impl Default for KeyType {
    fn default() -> Self {
        KeyType::RSA
    }
}

impl From<i32> for KeyType {
    fn from(i: i32) -> Self {
        match i {
            0 => KeyType::RSA,
            1 => KeyType::Ed25519,
            2 => KeyType::Secp256k1,
            3 => KeyType::ECDSA,
            _ => Self::default(),
        }
    }
}

impl<'a> From<&'a str> for KeyType {
    fn from(s: &'a str) -> Self {
        match s {
            "RSA" => KeyType::RSA,
            "Ed25519" => KeyType::Ed25519,
            "Secp256k1" => KeyType::Secp256k1,
            "ECDSA" => KeyType::ECDSA,
            _ => Self::default(),
        }
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct PublicKey {
    pub Type: keys_proto::KeyType,
    pub Data: Vec<u8>,
}

impl<'a> MessageRead<'a> for PublicKey {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.Type = r.read_enum(bytes)?,
                Ok(18) => msg.Data = r.read_bytes(bytes)?.to_owned(),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for PublicKey {
    fn get_size(&self) -> usize {
        0
        + 1 + sizeof_varint(*(&self.Type) as u64)
        + 1 + sizeof_len((&self.Data).len())
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        w.write_with_tag(8, |w| w.write_enum(*&self.Type as i32))?;
        w.write_with_tag(18, |w| w.write_bytes(&**&self.Data))?;
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct PrivateKey {
    pub Type: keys_proto::KeyType,
    pub Data: Vec<u8>,
}

impl<'a> MessageRead<'a> for PrivateKey {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.Type = r.read_enum(bytes)?,
                Ok(18) => msg.Data = r.read_bytes(bytes)?.to_owned(),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for PrivateKey {
    fn get_size(&self) -> usize {
        0
        + 1 + sizeof_varint(*(&self.Type) as u64)
        + 1 + sizeof_len((&self.Data).len())
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        w.write_with_tag(8, |w| w.write_enum(*&self.Type as i32))?;
        w.write_with_tag(18, |w| w.write_bytes(&**&self.Data))?;
        Ok(())
    }
}

