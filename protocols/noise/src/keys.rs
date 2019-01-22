// Copyright 2019 Parity Technologies (UK) Ltd.
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

use rand::Rng;
use x25519_dalek::{x25519, X25519_BASEPOINT_BYTES};

#[derive(Debug, Clone)]
pub enum Curve25519 {}

#[derive(Debug, Clone)]
pub struct PublicKey<T> {
    bytes: [u8; 32],
    _marker: std::marker::PhantomData<T>
}

impl<T> From<[u8; 32]> for PublicKey<T> {
    fn from(bytes: [u8; 32]) -> Self {
        PublicKey { bytes, _marker: std::marker::PhantomData }
    }
}

impl<T> AsRef<[u8]> for PublicKey<T> {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

pub struct SecretKey<T> {
    bytes: [u8; 32],
    _marker: std::marker::PhantomData<T>
}

impl<T> From<[u8; 32]> for SecretKey<T> {
    fn from(bytes: [u8; 32]) -> Self {
        SecretKey { bytes, _marker: std::marker::PhantomData }
    }
}

impl<T> AsRef<[u8]> for SecretKey<T> {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

pub struct Keypair<T> {
    secret: SecretKey<T>,
    public: PublicKey<T>
}

impl<T> Keypair<T> {
    pub fn new(s: SecretKey<T>, p: PublicKey<T>) -> Self {
        Keypair { secret: s, public: p }
    }

    pub fn secret(&self) -> &SecretKey<T> {
        &self.secret
    }

    pub fn public(&self) -> &PublicKey<T> {
        &self.public
    }
}

impl Keypair<Curve25519> {
    pub fn gen_curve25519() -> Self {
        let mut secret = [0; 32];
        rand::thread_rng().fill(&mut secret);
        let public = x25519(secret, X25519_BASEPOINT_BYTES);
        Keypair {
            secret: SecretKey {
                bytes: secret,
                _marker: std::marker::PhantomData
            },
            public: PublicKey {
                bytes: public,
                _marker: std::marker::PhantomData
            }
        }
    }
}

