
use aes_ctr::stream_cipher::generic_array::GenericArray;
use aes_ctr::stream_cipher::{NewFixStreamCipher, StreamCipherCore};
use aes_ctr::{Aes128Ctr, Aes256Ctr};

#[derive(Clone, Copy)]
pub enum KeySize {
    KeySize128,
    KeySize256,
}

/// Returns your stream cipher depending on `KeySize`.
#[cfg(not(all(feature = "aes-all", any(target_arch = "x86_64", target_arch = "x86"))))]
pub(crate) fn ctr(key_size: KeySize, key: &[u8], iv: &[u8]) -> Box<StreamCipherCore + 'static> {
    ctr_int(key_size, key, iv)
}
 
/// Returns your stream cipher depending on `KeySize`.
#[cfg(all(feature = "aes-all", any(target_arch = "x86_64", target_arch = "x86")))]
pub(crate) fn ctr(key_size: KeySize, key: &[u8], iv: &[u8]) -> Box<StreamCipherCore + 'static> {
    if *aes_alt::AES_NI {
        aes_alt::ctr_alt(key_size, key, iv)
    } else {
        ctr_int(key_size, key, iv)
    }
}


#[cfg(all(feature = "aes-all", any(target_arch = "x86_64", target_arch = "x86")))]
mod aes_alt {
    extern crate ctr;
    extern crate aesni;
    use self::ctr::Ctr128;
    use self::aesni::{Aes128, Aes256};
    use self::ctr::stream_cipher::{NewFixStreamCipher, StreamCipherCore};
    use self::ctr::stream_cipher::generic_array::GenericArray;
    use super::KeySize;

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
    pub fn ctr_alt(key_size: KeySize, key: &[u8], iv: &[u8]) -> Box<StreamCipherCore + 'static> {
        match key_size {
            KeySize::KeySize128 => Box::new(Aes128Ctr::new(
                GenericArray::from_slice(key),
                GenericArray::from_slice(iv),
            )),
            KeySize::KeySize256 => Box::new(Aes256Ctr::new(
                GenericArray::from_slice(key),
                GenericArray::from_slice(iv),
            )),
        }
    }

}

#[inline]
fn ctr_int(key_size: KeySize, key: &[u8], iv: &[u8]) -> Box<StreamCipherCore + 'static> {
    match key_size {
        KeySize::KeySize128 => Box::new(Aes128Ctr::new(
            GenericArray::from_slice(key),
            GenericArray::from_slice(iv),
        )),
        KeySize::KeySize256 => Box::new(Aes256Ctr::new(
            GenericArray::from_slice(key),
            GenericArray::from_slice(iv),
        )),
    }
}

#[cfg(all(
    feature = "aes-all", 
    any(target_arch = "x86_64", target_arch = "x86"),
))]
#[test]
fn assert_non_native_run() {
    // this test is for asserting aes unsuported opcode does not break on old cpu
    let key = [0;16];
    let iv = [0;16];
 
    let mut aes = ctr(KeySize::KeySize128, &key, &iv);
    let mut content = [0;16];
    assert!(aes
            .try_apply_keystream(&mut content).is_ok());
     
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
