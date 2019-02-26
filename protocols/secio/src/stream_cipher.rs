// Copyright 2018 Parity Technologies (UK) Ltd.
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

use super::codec::StreamCipher;
use aes_ctr::stream_cipher::generic_array::GenericArray;
use aes_ctr::stream_cipher::{NewStreamCipher, LoopError, SyncStreamCipher};
use aes_ctr::{Aes128Ctr, Aes256Ctr};
use ctr::Ctr128;
use twofish::Twofish;

/// Possible encryption ciphers.
#[derive(Clone, Copy, Debug)]
pub enum Cipher {
    Aes128,
    Aes256,
    TwofishCtr,
    Null,
}

impl Cipher {
    /// Returns the size of in bytes of the key expected by the cipher.
    pub fn key_size(&self) -> usize {
        match *self {
            Cipher::Aes128 => 16,
            Cipher::Aes256 => 32,
            Cipher::TwofishCtr => 32,
            Cipher::Null => 0,
        }
    }

    /// Returns the size of in bytes of the IV expected by the cipher.
    #[inline]
    pub fn iv_size(&self) -> usize {
        match self {
            Cipher::Aes128 | Cipher::Aes256 | Cipher::TwofishCtr => 16,
            Cipher::Null => 0
        }
    }
}

/// A no-op cipher which does not encrypt or decrypt at all.
/// Obviously only useful for debugging purposes.
#[derive(Clone, Copy, Debug)]
pub struct NullCipher;

impl SyncStreamCipher for NullCipher {
    fn try_apply_keystream(&mut self, _data: &mut [u8]) -> Result<(), LoopError> {
        Ok(())
    }
}

/// Returns your stream cipher depending on `Cipher`.
#[cfg(not(all(feature = "aes-all", any(target_arch = "x86_64", target_arch = "x86"))))]
pub fn ctr(key_size: Cipher, key: &[u8], iv: &[u8]) -> StreamCipher {
    ctr_int(key_size, key, iv)
}

/// Returns your stream cipher depending on `Cipher`.
#[cfg(all(feature = "aes-all", any(target_arch = "x86_64", target_arch = "x86")))]
pub fn ctr(key_size: Cipher, key: &[u8], iv: &[u8]) -> StreamCipher {
    if *aes_alt::AES_NI {
        aes_alt::ctr_alt(key_size, key, iv)
    } else {
        ctr_int(key_size, key, iv)
    }
}


#[cfg(all(feature = "aes-all", any(target_arch = "x86_64", target_arch = "x86")))]
mod aes_alt {
    use crate::codec::StreamCipher;
    use ctr::Ctr128;
    use aesni::{Aes128, Aes256};
    use ctr::stream_cipher::NewStreamCipher;
    use ctr::stream_cipher::generic_array::GenericArray;
    use lazy_static::lazy_static;
    use twofish::Twofish;
    use super::{Cipher, NullCipher};

    lazy_static! {
        pub static ref AES_NI: bool = is_x86_feature_detected!("aes")
            && is_x86_feature_detected!("sse2")
            && is_x86_feature_detected!("sse3");

   }

    /// AES-128 in CTR mode
    pub type Aes128Ctr = Ctr128<Aes128>;
    /// AES-256 in CTR mode
    pub type Aes256Ctr = Ctr128<Aes256>;
    /// Returns alternate stream cipher if target functionalities does not allow standard one.
    /// Eg : aes without sse
    pub fn ctr_alt(key_size: Cipher, key: &[u8], iv: &[u8]) -> StreamCipher {
        match key_size {
            Cipher::Aes128 => Box::new(Aes128Ctr::new(
                GenericArray::from_slice(key),
                GenericArray::from_slice(iv),
            )),
            Cipher::Aes256 => Box::new(Aes256Ctr::new(
                GenericArray::from_slice(key),
                GenericArray::from_slice(iv),
            )),
            Cipher::TwofishCtr => Box::new(Ctr128::<Twofish>::new(
                GenericArray::from_slice(key),
                GenericArray::from_slice(iv),
            )),
            Cipher::Null => Box::new(NullCipher),
        }
    }

}

#[inline]
fn ctr_int(key_size: Cipher, key: &[u8], iv: &[u8]) -> StreamCipher {
    match key_size {
        Cipher::Aes128 => Box::new(Aes128Ctr::new(
            GenericArray::from_slice(key),
            GenericArray::from_slice(iv),
        )),
        Cipher::Aes256 => Box::new(Aes256Ctr::new(
            GenericArray::from_slice(key),
            GenericArray::from_slice(iv),
        )),
        Cipher::TwofishCtr => Box::new(Ctr128::<Twofish>::new(
            GenericArray::from_slice(key),
            GenericArray::from_slice(iv),
        )),
        Cipher::Null => Box::new(NullCipher),
    }
}

#[cfg(all(
        feature = "aes-all",
        any(target_arch = "x86_64", target_arch = "x86"),
))]
#[cfg(test)]
mod tests {
    use super::{Cipher, ctr};

    #[test]
    fn assert_non_native_run() {
        // this test is for asserting aes unsuported opcode does not break on old cpu
        let key = [0;16];
        let iv = [0;16];

        let mut aes = ctr(Cipher::Aes128, &key, &iv);
        let mut content = [0;16];
        aes.encrypt(&mut content);

    }
}

// aesni compile check for aes-all (aes-all import aesni through aes_ctr only if those checks pass)
#[cfg(all(
    feature = "aes-all",
    any(target_arch = "x86_64", target_arch = "x86"),
    any(target_feature = "aes", target_feature = "ssse3"),
))]
compile_error!(
    "aes-all must be compile without aes and sse3 flags : currently \
    is_x86_feature_detected macro will not detect feature correctly otherwhise. \
    RUSTFLAGS=\"-C target-feature=+aes,+ssse3\" enviromental variable. \
    For x86 target arch additionally enable sse2 target feature."
);
