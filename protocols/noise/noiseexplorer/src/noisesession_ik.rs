/* ---------------------------------------------------------------- *
 * PROCESSES                                                        *
 * ---------------------------------------------------------------- */

use crate::{consts::HASHLEN,
            error::NoiseError,
            state_ik::{CipherState, HandshakeState},
            types::{Hash, Keypair, Message, Psk, PublicKey}};
/// A `NoiseSession` object is used to keep track of the states of both local
/// and remote parties before, during, and after a handshake.
///
/// It contains:
/// - `h`:  Stores the handshake hash output after a successful handshake in a
///   Hash object. Is initialized as array of 0 bytes.
/// - `mc`:  Keeps track of the total number of incoming and outgoing messages,
///   including those sent during a handshake.
/// - `i`: `bool` value that indicates whether this session corresponds to the
///   local or remote party.
/// - `hs`: Keeps track of the local party's state while a handshake is being
///   performed.
/// - `cs1`: Keeps track of the local party's post-handshake state. Contains a
///   cryptographic key and a nonce.
/// - `cs2`: Keeps track of the remote party's post-handshake state. Contains a
///   cryptographic key and a nonce.
#[derive(Clone)]
pub struct NoiseSession {
	hs:  HandshakeState,
	h:   Hash,
	cs1: CipherState,
	cs2: CipherState,
	mc:  u128,
	i:   bool,
}
impl NoiseSession {
	/// Clears `cs1`.
	pub fn clear_local_cipherstate(&mut self,) {
		self.cs1.clear();
	}

	/// Clears `cs2`.
	pub fn clear_remote_cipherstate(&mut self,) {
		self.cs2.clear();
	}

	/// `NoiseSession` destructor function.
	pub fn end_session(mut self,) {
		self.hs.clear();
		self.clear_local_cipherstate();
		self.clear_remote_cipherstate();
		self.cs2.clear();
		self.mc = 0;
		self.h = Hash::new();
	}

	/// Returns `h`.
	pub fn get_handshake_hash(&self,) -> [u8; HASHLEN] {
		self.h.as_bytes()
	}

	/// Sets the value of the local ephemeral keypair as the parameter `e`.
	pub fn set_ephemeral_keypair(&mut self, e: Keypair,) {
		self.hs.set_ephemeral_keypair(e,);
	}

    pub fn get_remote_static_public_key(&self) -> PublicKey {
         self.hs.get_remote_static_public_key()
     }

	/// Instantiates a `NoiseSession` object. Takes the following as parameters:
	/// - `initiator`: `bool` variable. To be set as `true` when initiating a handshake with a remote party, or `false` otherwise.
	/// - `prologue`: `Message` object. Could optionally contain the name of the protocol to be used.
	/// - `s`: `Keypair` object. Contains local party's static keypair.
	/// - `rs`: `PublicKey` object. Contains the remote party's static public key.

	pub fn init_session(initiator: bool, prologue: Message, s: Keypair, rs: PublicKey) -> NoiseSession {
		if initiator {
			NoiseSession{
				hs: HandshakeState::initialize_initiator(&prologue.as_bytes(), s, rs, Psk::new()),
				mc: 0,
				i: initiator,
				cs1: CipherState::new(),
				cs2: CipherState::new(),
				h: Hash::new(),
			}
		} else {
			NoiseSession {
				hs: HandshakeState::initialize_responder(&prologue.as_bytes(), s, rs, Psk::new()),
				mc: 0,
				i: initiator,
				cs1: CipherState::new(),
				cs2: CipherState::new(),
				h: Hash::new(),
			}
		}
	}

	/// Takes a `Message` object containing plaintext as a parameter.
	/// Returns a `Ok(Message)` object containing the corresponding ciphertext upon successful encryption, and `Err(NoiseError)` otherwise
	///
	/// _Note that while `mc` <= 1 the ciphertext will be included as a payload for handshake messages and thus will not offer the same guarantees offered by post-handshake messages._
	pub fn send_message(&mut self, message: Message) -> Result<Message, NoiseError> {
		let out: Vec<u8>;
		if self.mc == 0 {
			out = self.hs.write_message_a(message.as_bytes())?;
		}
		else if self.mc == 1 {
			let temp = self.hs.write_message_b(message.as_bytes())?;
			self.h = temp.0;
			self.cs1 = temp.2;
			self.cs2 = temp.3;
			self.hs.clear();
			out = temp.1;
		} else if self.i {
			out = self.cs1.write_message_regular(message.as_bytes())?;
		} else {
			out = self.cs2.write_message_regular(message.as_bytes())?;
		}
		let out: Message = Message::from_vec(out)?;
		self.mc += 1;
		Ok(out)
	}

	/// Takes a `Message` object received from the remote party as a parameter.
	/// Returns a `Ok(Message)` object containing plaintext upon successful decryption, and `Err(NoiseError)` otherwise.
	///
	/// _Note that while `mc` <= 1 the ciphertext will be included as a payload for handshake messages and thus will not offer the same guarantees offered by post-handshake messages._
	pub fn recv_message(&mut self, message: Message) -> Result<Message, NoiseError> {
		let out: Vec<u8>;
		if self.mc == 0 {
			out = self.hs.read_message_a(message.as_bytes())?;
		}
		else if self.mc == 1 {
			let temp = self.hs.read_message_b(message.as_bytes())?;
				self.h = temp.0;
				self.cs1 = temp.2;
				self.cs2 = temp.3;
				self.hs.clear();
				out = temp.1;
		} else if self.i {
			out = self.cs2.read_message_regular(message.as_bytes())?;
		} else {
				out = self.cs1.read_message_regular(message.as_bytes())?;
		}
		let out: Message = Message::from_vec(out)?;
		self.mc += 1;
		Ok(out)
	}
}
