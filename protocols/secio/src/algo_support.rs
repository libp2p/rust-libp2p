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

macro_rules! supported_impl {
    ($mod_name:ident: $ty:ty, $($name:expr => $val:expr),*,) => (
        pub mod $mod_name {
            use std::cmp::Ordering;
            #[allow(unused_imports)]
            use stream_cipher::Cipher;
            #[allow(unused_imports)]
            use ring::{agreement, digest};
            use error::SecioError;

            /// String to advertise to the remote.
            pub const PROPOSITION_STRING: &'static str = concat_comma!($($name),*);

            /// Choose which algorithm to use based on the remote's advertised list.
            pub fn select_best(hashes_ordering: Ordering, input: &str) -> Result<$ty, SecioError> {
                match hashes_ordering {
                    Ordering::Less | Ordering::Equal => {
                        for second_elem in input.split(',') {
                            $(
                                if $name == second_elem {
                                    return Ok($val);
                                }
                            )+
                        }
                    },
                    Ordering::Greater => {
                        $(
                            for second_elem in input.split(',') {
                                if $name == second_elem {
                                    return Ok($val);
                                }
                            }
                        )+
                    },
                };

                Err(SecioError::NoSupportIntersection(PROPOSITION_STRING, input.to_owned()))
            }
        }
    );
}

// Concatenates several strings with commas.
macro_rules! concat_comma {
    ($first:expr, $($rest:expr),*) => (
        concat!($first $(, ',', $rest)*)
    );
}

// TODO: there's no library in the Rust ecosystem that supports P-521, but the Go & JS
//       implementations advertise it
supported_impl!(
    exchanges: &'static agreement::Algorithm,
    "P-256" => &agreement::ECDH_P256,
    "P-384" => &agreement::ECDH_P384,
);

// TODO: the Go & JS implementations advertise Blowfish ; however doing so in Rust leads to
//       runtime errors
supported_impl!(
    ciphers: Cipher,
    "AES-128" => Cipher::Aes128,
    "AES-256" => Cipher::Aes256,
    "TwofishCTR" => Cipher::Twofish,
);

supported_impl!(
    hashes: &'static digest::Algorithm,
    "SHA256" => &digest::SHA256,
    "SHA512" => &digest::SHA512,
);
