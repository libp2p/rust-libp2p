//! This module contains some utilities for algorithm support exchange.

macro_rules! supported_impl {
    ($mod_name:ident: $ty:ty, $($name:expr => $val:expr),*,) => (
        pub mod $mod_name {
            use std::cmp::Ordering;
            #[allow(unused_imports)]
            use crypto::aes::KeySize;
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

macro_rules! concat_comma {
    ($first:expr, $($rest:expr),*) => (
        concat!($first $(, ',', $rest)*)
    );
}

// TODO: there's no library in the ecosystem that supports P-521, but the Go & JS implementations
//       advertise it
supported_impl!(
    exchanges: &'static agreement::Algorithm,
    "P-256" => &agreement::ECDH_P256,
    "P-384" => &agreement::ECDH_P384,
);

// TODO: using Blowfish leads to runtime errors
supported_impl!(
    ciphers: KeySize,
    "AES-128" => KeySize::KeySize128,
    "AES-256" => KeySize::KeySize256,
);

supported_impl!(
    hashes: &'static digest::Algorithm,
    "SHA256" => &digest::SHA256,
    "SHA512" => &digest::SHA512,
);
