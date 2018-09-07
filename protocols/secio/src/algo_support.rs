// Copyright 2017 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

//! This module contains some utilities for algorithm support exchange.
//!
//! One important part of the SECIO handshake is negotiating algorithms. This is what this module
//! helps you with.

use error::SecioError;
use libp2p_core::Endpoint;
use ring::{agreement, digest};
use stream_cipher::Cipher;

pub(crate) const DEFAULT_AGREEMENTS_PROPOSITION: &str = "P-256,P384";
pub(crate) const DEFAULT_CIPHERS_PROPOSITION: &str = "AES-128,AES-256,NULL";
pub(crate) const DEFAULT_DIGESTS_PROPOSITION: &str = "SHA256,SHA512";

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum KeyAgreement {
    EcdhP256,
    EcdhP384
}

pub fn key_agreements_proposition<'a, I>(xchgs: I) -> String
where
    I: IntoIterator<Item=&'a KeyAgreement>
{
    let mut s = String::new();
    for x in xchgs {
        match x {
            KeyAgreement::EcdhP256 => s.push_str("P-256,"),
            KeyAgreement::EcdhP384 => s.push_str("P-384,")
        }
    }
    s.pop(); // remove trailing comma if any
    s
}

pub fn select_agreement<'a>(r: Endpoint, ours: &str, theirs: &str) -> Result<&'a agreement::Algorithm, SecioError> {
    let (a, b) = match r {
        Endpoint::Dialer => (ours, theirs),
        Endpoint::Listener => (theirs, ours)
    };
    for x in a.split(',') {
        if b.split(',').any(|y| x == y) {
            match x {
                "P-256" => return Ok(&agreement::ECDH_P256),
                "P-384" => return Ok(&agreement::ECDH_P384),
                _ => continue
            }
        }
    }
    Err(SecioError::NoSupportIntersection)
}


pub fn ciphers_proposition<'a, I>(ciphers: I) -> String
where
    I: IntoIterator<Item=&'a Cipher>
{
    let mut s = String::new();
    for c in ciphers {
        match c {
            Cipher::Aes128 => s.push_str("AES-128,"),
            Cipher::Aes256 => s.push_str("AES-256,"),
            Cipher::Null => s.push_str("NULL,")
        }
    }
    s.pop(); // remove trailing comma if any
    s
}

pub fn select_cipher(r: Endpoint, ours: &str, theirs: &str) -> Result<Cipher, SecioError> {
    let (a, b) = match r {
        Endpoint::Dialer => (ours, theirs),
        Endpoint::Listener => (theirs, ours)
    };
    for x in a.split(',') {
        if b.split(',').any(|y| x == y) {
            match x {
                "AES-128" => return Ok(Cipher::Aes128),
                "AES-256" => return Ok(Cipher::Aes256),
                "NULL" => return Ok(Cipher::Null),
                _ => continue
            }
        }
    }
    Err(SecioError::NoSupportIntersection)
}


#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Digest {
    Sha256,
    Sha512
}

pub fn digests_proposition<'a, I>(digests: I) -> String
where
    I: IntoIterator<Item=&'a Digest>
{
    let mut s = String::new();
    for d in digests {
        match d {
            Digest::Sha256 => s.push_str("SHA256,"),
            Digest::Sha512 => s.push_str("SHA512,")
        }
    }
    s.pop(); // remove trailing comma if any
    s
}

pub fn select_digest<'a>(r: Endpoint, ours: &str, theirs: &str) -> Result<&'a digest::Algorithm, SecioError> {
    let (a, b) = match r {
        Endpoint::Dialer => (ours, theirs),
        Endpoint::Listener => (theirs, ours)
    };
    for x in a.split(',') {
        if b.split(',').any(|y| x == y) {
            match x {
                "SHA256" => return Ok(&digest::SHA256),
                "SHA512" => return Ok(&digest::SHA512),
                _ => continue
            }
        }
    }
    Err(SecioError::NoSupportIntersection)
}

