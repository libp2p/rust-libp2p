/* ---------------------------------------------------------------- *
 * TYPES                                                            *
 * ---------------------------------------------------------------- */

use crate::{
    consts::{DHLEN, EMPTY_KEY, HASHLEN, MAX_MESSAGE, MAX_NONCE},
    error::NoiseError,
};
use hacl_star::curve25519;

use rand;
use zeroize::Zeroize;

fn decode_str_32(s: &str) -> Result<[u8; DHLEN], NoiseError> {
    if let Ok(x) = hex::decode(s) {
        if x.len() == DHLEN {
            let mut temp: [u8; DHLEN] = [0_u8; DHLEN];
            temp.copy_from_slice(&x[..]);
            Ok(temp)
        }
        else {
            return Err(NoiseError::InvalidInputError);
        }
    }
    else {
        return Err(NoiseError::InvalidInputError);
    }
}

fn decode_str(s: &str) -> Result<Vec<u8>, NoiseError> {
    let res = hex::decode(s)?;
    Ok(res)
}

#[derive(Clone)]
pub(crate) struct Hash {
    h: [u8; HASHLEN],
}
impl Hash {
    pub(crate) fn clear(&mut self) {
        self.h.zeroize();
    }
    pub(crate) fn from_bytes(hash: [u8; HASHLEN]) -> Self {
        Self {
            h: hash,
        }
    }
    pub(crate) fn as_bytes(&self) -> [u8; DHLEN] {
        self.h
    }
    pub(crate) fn new() -> Self {
        Self::from_bytes([0_u8; HASHLEN])
    }
}

#[derive(Clone, Default)]
pub struct Key {
    k: [u8; DHLEN],
}
impl Key {
    pub(crate) fn clear(&mut self) {
        self.k.zeroize();
    }
    /// Instanciates a new empty `Key`.
    pub fn new() -> Self {
        Self::from_bytes(EMPTY_KEY)
    }
    /// Instanciates a new `Key` from an array of `DHLEN` bytes.
    pub fn from_bytes(key: [u8; DHLEN]) -> Self {
        Self {
            k: key,
        }
    }
    /// Instanciates a new `Key` from a string of hexadecimal values.
    /// # Example
    ///
    /// ```
    /// # use noiseexplorer_xx::{
    /// #   error::NoiseError,
    /// #   types::Key,
    /// # };
    /// # fn try_main() -> Result<(), NoiseError> {
    ///     let k = Key::from_str("4a3acbfdb163dec651dfa3194dece676d437029c62a408b4c5ea9114246e4893")?;
    /// #   Ok(())
    /// # }
    /// # fn main() {
    /// #   try_main().unwrap();
    /// # }
    /// ```
    pub fn from_str(key: &str) -> Result<Self, NoiseError> {
        let a = decode_str_32(key)?;
        Ok(Self::from_bytes(a))
    }
    pub(crate) fn as_bytes(&self) -> [u8; DHLEN] {
        self.k
    }
    /// Checks whether a `Key` object is empty or not.
    /// # Example
    ///
    /// ```
    /// # use noiseexplorer_xx::{
    /// #   error::NoiseError,
    /// #   types::Key,
    /// # };
    /// # fn try_main() -> Result<(), NoiseError> {
    ///     let empty_key1 = Key::from_str("0000000000000000000000000000000000000000000000000000000000000000")?;
    ///     let empty_key2 = Key::new();
    ///     let k = Key::from_str("4a3acbfdb163dec651dfa3194dece676d437029c62a408b4c5ea9114246e4893")?;
    ///     assert!(empty_key1.is_empty());
    ///     assert!(empty_key2.is_empty());
    ///     assert!(!k.is_empty());
    /// #   Ok(())
    /// # }
    /// # fn main() {
    /// #   try_main().unwrap();
    /// # }
    /// ```
    pub fn is_empty(&self) -> bool {
        crypto::util::fixed_time_eq(&self.k[..], &EMPTY_KEY)
    }
    /// Derives a `PublicKey` from the `Key` and returns it.
    pub fn generate_public_key(private_key: &[u8; DHLEN]) -> PublicKey {
        let mut output: [u8; DHLEN] = EMPTY_KEY;
        output.copy_from_slice(private_key);
        let output = curve25519::SecretKey(output).get_public();
        PublicKey {
            k: output.0,
        }
    }
}

pub struct Psk {
    psk: [u8; DHLEN],
}
impl Psk {
    /// Instanciates a new empty `Psk`.
    pub fn new() -> Self {
        Self::from_bytes(EMPTY_KEY)
    }
    pub(crate) fn clear(&mut self) {
        self.psk.zeroize();
    }
    /// Instanciates a new `Psk` from an array of `DHLEN` bytes.
    pub fn from_bytes(k: [u8; DHLEN]) -> Self {
        Self {
            psk: k,
        }
    }
    /// Instanciates a new `Psk` from a string of hexadecimal values.
    /// # Example
    ///
    /// ```
    /// # use noiseexplorer_xx::{
    /// #   error::NoiseError,
    /// #   types::Psk,
    /// # };
    /// # fn try_main() -> Result<(), NoiseError> {
    ///     let k = Psk::from_str("4a3acbfdb163dec651dfa3194dece676d437029c62a408b4c5ea9114246e4893")?;
    /// #   Ok(())
    /// # }
    /// # fn main() {
    /// #   try_main().unwrap();
    /// # }
    /// ```
    pub fn from_str(k: &str) -> Result<Self, NoiseError> {
        let psk = decode_str_32(k)?;
        Ok(Self::from_bytes(psk))
    }
    #[allow(dead_code)]
    pub(crate) fn as_bytes(&self) -> [u8; DHLEN] {
        self.psk
    }
    /// Checks whether a `Psk` object is empty or not.
    /// # Example
    ///
    /// ```
    /// # use noiseexplorer_xx::{
    /// #   error::NoiseError,
    /// #   types::Psk,
    /// # };
    /// # fn try_main() -> Result<(), NoiseError> {
    ///     let empty_key1 = Psk::from_str("0000000000000000000000000000000000000000000000000000000000000000")?;
    ///     let empty_key2 = Psk::new();
    ///     let k = Psk::from_str("4a3acbfdb163dec651dfa3194dece676d437029c62a408b4c5ea9114246e4893")?;
    ///     assert!(empty_key1.is_empty());
    ///     assert!(empty_key2.is_empty());
    ///     assert!(!k.is_empty());
    /// #   Ok(())
    /// # }
    /// # fn main() {
    /// #   try_main().unwrap();
    /// # }
    /// ```
    pub fn is_empty(&self) -> bool {
        crypto::util::fixed_time_eq(&self.psk[..], &EMPTY_KEY)
    }
}
impl Clone for Psk {
    fn clone(&self) -> Self {
        Self {
            psk: self.as_bytes().to_owned(),
        }
    }
}

#[derive(Clone)]
pub struct PrivateKey {
    k: [u8; DHLEN],
}
impl PrivateKey {
    pub(crate) fn clear(&mut self) {
        self.k.zeroize();
    }
    /// Instanciates a new empty `PrivateKey`.
    pub fn empty() -> Self {
        Self {
            k: EMPTY_KEY,
        }
    }
    /// Instanciates a new `PrivateKey` from an array of `DHLEN` bytes.
    pub fn from_bytes(k: [u8; DHLEN]) -> Self {
        Self::from_hacl_secret_key(curve25519::SecretKey(k))
    }
    pub(crate) fn from_hacl_secret_key(hacl_secret: curve25519::SecretKey) -> Self {
        Self {
            k: hacl_secret.0,
        }
    }
    /// Instanciates a new `PrivateKey` from a string of hexadecimal values.
    /// # Example
    ///
    /// ```
    /// # use noiseexplorer_xx::{
    /// #   error::NoiseError,
    /// #   types::PrivateKey,
    /// # };
    /// # fn try_main() -> Result<(), NoiseError> {
    ///     let k = PrivateKey::from_str("4a3acbfdb163dec651dfa3194dece676d437029c62a408b4c5ea9114246e4893")?;
    /// #   Ok(())
    /// # }
    /// # fn main() {
    /// #   try_main().unwrap();
    /// # }
    /// ```
    pub fn from_str(key: &str) -> Result<Self, NoiseError> {
        let k = decode_str_32(key)?;
        Ok(Self::from_hacl_secret_key(curve25519::SecretKey(k)))
    }
    pub(crate) fn as_bytes(&self) -> [u8; DHLEN] {
        self.k
    }
    /// Checks whether a `PrivateKey` object is empty or not.
    /// # Example
    ///
    /// ```
    /// # use noiseexplorer_xx::{
    /// #   error::NoiseError,
    /// #   types::PrivateKey,
    /// # };
    /// # fn try_main() -> Result<(), NoiseError> {
    ///     let empty_key1 = PrivateKey::from_str("0000000000000000000000000000000000000000000000000000000000000000")?;
    ///     let empty_key2 = PrivateKey::empty();
    ///     let k = PrivateKey::from_str("4a3acbfdb163dec651dfa3194dece676d437029c62a408b4c5ea9114246e4893")?;
    ///     assert!(empty_key1.is_empty());
    ///     assert!(empty_key2.is_empty());
    ///     assert!(!k.is_empty());
    /// #   Ok(())
    /// # }
    /// # fn main() {
    /// #   try_main().unwrap();
    /// # }
    /// ```
    pub fn is_empty(&self) -> bool {
        crypto::util::fixed_time_eq(&self.k[..], &EMPTY_KEY)
    }
    /// Derives a `PublicKey` from the `PrivateKey` then returns `Ok(PublicKey)` when successful and `Err(NoiseError)` otherwise.
    pub fn generate_public_key(&self) -> Result<PublicKey, NoiseError> {
        if self.is_empty() {
            return Err(NoiseError::InvalidKeyError);
        }
        Ok(PublicKey {
            k: curve25519::SecretKey(self.k).get_public().0,
        })
    }
}

#[derive(Copy, Clone)]
pub struct PublicKey {
    k: [u8; DHLEN],
}
impl PublicKey {
    /// Instanciates a new empty `PublicKey`.
    pub fn empty() -> Self {
        Self {
            k: EMPTY_KEY,
        }
    }
    /// Instanciates a new `PublicKey` from an array of `DHLEN` bytes.
    pub fn from_bytes(k: [u8; DHLEN]) -> Self {
        Self {
            k,
        }
    }
    pub(crate) fn clear(&mut self) {
        self.k.zeroize();
    }
    /// Instanciates a new `PublicKey` from a string of hexadecimal values.
    /// Returns `Ok(PublicKey)` when successful and `Err(NoiseError)` otherwise.
    /// # Example
    ///
    /// ```
    /// # use noiseexplorer_xx::{
    /// #   error::NoiseError,
    /// #   types::PublicKey,
    /// # };
    /// # fn try_main() -> Result<(), NoiseError> {
    ///     let k = PublicKey::from_str("4a3acbfdb163dec651dfa3194dece676d437029c62a408b4c5ea9114246e4893")?;
    ///     println!("{:?}", k.as_bytes());
    /// #   Ok(())
    /// # }
    /// # fn main() {
    /// #   try_main().unwrap();
    /// # }
    /// ```
    pub fn from_str(key: &str) -> Result<Self, NoiseError> {
        let pk = decode_str_32(key)?;
        Ok(Self::from_hacl_public_key(curve25519::PublicKey(pk)))
    }
    pub(crate) fn from_hacl_public_key(hacl_public: curve25519::PublicKey) -> Self {
        Self {
            k: hacl_public.0,
        }
    }
    pub fn as_bytes(&self) -> [u8; DHLEN] {
        self.k
    }
    /// Checks whether a `PublicKey` object is empty or not.
    /// # Example
    ///
    /// ```
    /// # use noiseexplorer_xx::{
    /// #   error::NoiseError,
    /// #   types::PublicKey,
    /// # };
    /// # fn try_main() -> Result<(), NoiseError> {
    ///     let empty_key1 = PublicKey::from_str("0000000000000000000000000000000000000000000000000000000000000000")?;
    ///     let empty_key2 = PublicKey::empty();
    ///     let k = PublicKey::from_str("4a3acbfdb163dec651dfa3194dece676d437029c62a408b4c5ea9114246e4893")?;
    ///     assert!(empty_key1.is_empty());
    ///     assert!(empty_key2.is_empty());
    ///     assert!(!k.is_empty());
    /// #   Ok(())
    /// # }
    /// # fn main() {
    /// #   try_main().unwrap();
    /// # }
    /// ```
    pub fn is_empty(&self) -> bool {
        crypto::util::fixed_time_eq(&self.k[..], &EMPTY_KEY)
    }
}

#[derive(Copy, Clone)]
pub(crate) struct Nonce {
    n: u64,
}
impl Nonce {
    pub(crate) fn new() -> Self {
        Self {
            n: 0_u64,
        }
    }
    pub(crate) fn increment(&mut self) {
        self.n += 1;
    }
    pub(crate) fn get_value(self) -> Result<u64, NoiseError> {
        if self.n == MAX_NONCE {
            return Err(NoiseError::ExhaustedNonceError);
        }
        Ok(self.n)
    }
}

#[derive(Clone)]
/// Data structure to be used
pub(crate) struct MessageBuffer {
    pub(crate) ne: [u8; DHLEN],
    pub(crate) ns: Vec<u8>,
    pub(crate) ciphertext: Vec<u8>,
}

pub struct Message {
    payload: Vec<u8>,
}
impl Message {
    /// Instanciates a new `Message` from a `Vec<u8>`.
    pub fn from_vec(m: Vec<u8>) -> Result<Self, NoiseError> {
        if m.len() > MAX_MESSAGE || m.is_empty() {
            return Err(NoiseError::UnsupportedMessageLengthError);
        }
        Ok(Self {
            payload: m,
        })
    }
    /// Instanciates a new `Message` from a `&str`.
    pub fn from_str(m: &str) -> Result<Self, NoiseError> {
        let msg = decode_str(m)?;
        Self::from_vec(msg)
    }
    /// Instanciates a new `Message` from a `&[u8]`.
    /// Returns `Ok(Message)` when successful and `Err(NoiseError)` otherwise.
    pub fn from_bytes(m: &[u8]) -> Result<Self, NoiseError> {
        Self::from_vec(Vec::from(m))
    }
    /// View the `Message` payload as a `Vec<u8>`.
    pub fn as_bytes(&self) -> &Vec<u8> {
        &self.payload
    }
    /// Returns a `usize` value that represents the `Message` payload length in bytes.
    pub fn len(&self) -> usize {
        self.payload.len()
    }
}
impl Clone for Message {
    fn clone(&self) -> Self {
        Self {
            payload: self.as_bytes().to_owned(),
        }
    }
}
impl PartialEq for Message {
    fn eq(&self, other: &Self) -> bool {
        self.payload == other.payload
    }
}
impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "({:X?})", self.payload)
    }
}

#[derive(Clone)]
pub struct Keypair {
    private_key: PrivateKey,
    public_key: PublicKey,
}
impl Keypair {
    pub fn clear(&mut self) {
        self.private_key.clear();
        self.public_key.clear();
    }
    /// Instanciates a `Keypair` where the `PrivateKey` and `PublicKey` fields are filled with 0 bytes.
    pub fn new_empty() -> Self {
        Self {
            private_key: PrivateKey::empty(),
            public_key: PublicKey::empty(),
        }
    }
    /// Instanciates a `Keypair` by generating a `PrivateKey` from random values using `thread_rng()`, then deriving the corresponding `PublicKey`
    pub fn new() -> Self {
        let hacl_keypair: (curve25519::SecretKey, curve25519::PublicKey) =
            curve25519::keypair(rand::thread_rng());
        Self {
            private_key: PrivateKey::from_hacl_secret_key(hacl_keypair.0),
            public_key: PublicKey::from_hacl_public_key(hacl_keypair.1),
        }
    }

    pub(crate) fn dh(&self, public_key: &[u8; DHLEN]) -> [u8; DHLEN] {
        let mut output: [u8; DHLEN] = EMPTY_KEY;
        curve25519::scalarmult(&mut output, &self.private_key.as_bytes(), public_key);
        output
    }
    /// Checks if the `PrivateKey` field of a `Keypair` is empty and returns either `true` or `false` accordingly.
    pub fn is_empty(&self) -> bool {
        self.private_key.is_empty()
    }
    /// Derives a `PublicKey` from a `Key` object.
    /// Returns a `Ok(Keypair)` containing the previous two values and `Err(NoiseError)` otherwise.
    pub fn from_key(k: PrivateKey) -> Result<Self, NoiseError> {
        let public_key: PublicKey = k.generate_public_key()?;
        Ok(Self {
            private_key: k,
            public_key,
        })
    }
    /// Derives a `PublicKey` from a `PrivateKey`.
    /// Returns a `Ok(Keypair)` containing the previous two values and `Err(NoiseError)` otherwise.
    pub fn from_private_key(k: PrivateKey) -> Result<Self, NoiseError> {
        Self::from_key(k)
    }
    /// Returns the `PublicKey` value from the `Keypair`
    pub fn get_public_key(&self) -> PublicKey {
        self.public_key
    }
}
