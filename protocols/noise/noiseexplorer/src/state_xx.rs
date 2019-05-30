/* ---------------------------------------------------------------- *
 * STATE MANAGEMENT                                                 *
 * ---------------------------------------------------------------- */

use crate::{consts::{DHLEN, EMPTY_HASH, EMPTY_KEY, HASHLEN, MAC_LENGTH, NONCE_LENGTH, ZEROLEN},
			error::NoiseError,
			prims::{decrypt, encrypt, hash, hash_with_context, hkdf},
			types::{Hash, Key, Keypair, Nonce, Psk, PublicKey},
			utils::from_slice_hashlen};
use hacl_star::chacha20poly1305;

#[derive(Clone)]
pub(crate) struct CipherState {
	k: Key,
	n: Nonce,
}

impl CipherState {
	pub(crate) fn new() -> Self {
		Self::from_key(Key::new())
	}

	pub(crate) fn clear_key(&mut self) {
		self.k.clear();
	}

	pub(crate) fn clear(&mut self) {
		self.k.clear();
		self.n = Nonce::new();
	}

	pub(crate) fn from_key(key: Key) -> Self {
		let nonce: Nonce = Nonce::new();
		Self {
			k: key,
			n: nonce,
		}
	}

	pub(crate) fn has_key(&self) -> bool {
		!self.k.is_empty()
	}

	#[allow(dead_code)]
	pub(crate) fn set_nonce(&mut self, n: Nonce) {
		self.n = n;
	}

	#[allow(dead_code)]
	pub(crate) fn get_nonce(&self) -> Nonce {
		self.n
	}

	pub(crate) fn encrypt_with_ad(
		&mut self, ad: &[u8], in_out: &mut [u8], mac: &mut [u8; MAC_LENGTH]) -> Result<(), NoiseError> {
		let nonce = self.n.get_value()?;
		if self.has_key() {
			encrypt(from_slice_hashlen(&self.k.as_bytes()[..]), nonce, ad, in_out, mac);
			self.n.increment();
			return Ok(());
		}
			Err(NoiseError::EmptyKeyError)
	}

	pub(crate) fn decrypt_with_ad(
		&mut self, ad: &[u8], in_out: &mut [u8], mac: &mut [u8; MAC_LENGTH]
	) -> Result<(), NoiseError> {
		let nonce = self.n.get_value()?;
		if self.has_key() {
			if decrypt(from_slice_hashlen(&self.k.as_bytes()[..]), nonce, ad, in_out, mac) {
				self.n.increment();
				return Ok(());
			}
			return Err(NoiseError::DecryptionError);
		}
			Err(NoiseError::EmptyKeyError)
	}

	#[allow(dead_code)]
	pub(crate) fn rekey(&mut self) {
		let mut in_out = EMPTY_KEY;
		chacha20poly1305::key(&self.k.as_bytes()).nonce(&[0xFFu8; NONCE_LENGTH]).encrypt(
			&ZEROLEN[..],
			&mut in_out[..],
			&mut [0u8; 16],
		);
		self.k.clear();
		self.k = Key::from_bytes(in_out);
	}

	pub(crate) fn write_message_regular(
		&mut self, in_out: &mut [u8]
	) -> Result<(), NoiseError> {
		let (in_out, mac) = in_out.split_at_mut(in_out.len()-MAC_LENGTH);
		let mut temp_mac: [u8; MAC_LENGTH] = [0u8; MAC_LENGTH];
		self.encrypt_with_ad(&ZEROLEN[..], in_out, &mut temp_mac)?;
		mac.copy_from_slice(&temp_mac[..]);
		Ok(())
	}

	pub(crate) fn read_message_regular(
		&mut self, in_out: &mut [u8]
	) -> Result<(), NoiseError> {
		let (in_out, mac) = in_out.split_at_mut(in_out.len()-MAC_LENGTH);
		let mut temp_mac: [u8; MAC_LENGTH] = [0u8; MAC_LENGTH];
		temp_mac.copy_from_slice(mac);
		self.decrypt_with_ad(&ZEROLEN[..], in_out, &mut temp_mac)?;
		temp_mac.copy_from_slice(&[0u8; MAC_LENGTH][..]);
		Ok(())
	}
}
#[derive(Clone)]
pub struct SymmetricState {
	cs: CipherState,
	ck: Hash,
	h:  Hash,
}

impl SymmetricState {
	pub(crate) fn clear(&mut self) {
		self.cs.clear_key();
		self.ck.clear();
	}

	pub fn initialize_symmetric(protocol_name: &[u8]) -> Self {
		let h: Hash;
		match protocol_name.len() {
			0..=31 => {
				let mut temp = [0u8; HASHLEN];
				let (protocol_name_len, _) = temp.split_at_mut(protocol_name.len());
				protocol_name_len.copy_from_slice(protocol_name);
				h = Hash::from_bytes(from_slice_hashlen(&temp[..]));
			},
			32 => h = Hash::from_bytes(from_slice_hashlen(protocol_name)),
			_ => h = Hash::from_bytes(hash(protocol_name)),
		}
		let ck: Hash = Hash::from_bytes(from_slice_hashlen(&h.as_bytes()[..]));
		let cs: CipherState = CipherState::new();
		Self {
			cs,
			ck,
			h,
		}
	}

	pub(crate) fn mix_key(&mut self, input_key_material: &[u8]) {
		let mut out0: [u8; HASHLEN] = EMPTY_HASH;
		let mut out1: [u8; HASHLEN] = EMPTY_HASH;
		let mut out2: [u8; HASHLEN] = EMPTY_HASH;
		hkdf(
			&self.ck.as_bytes()[..],
			input_key_material,
			2,
			&mut out0[..],
			&mut out1[..],
			&mut out2[..],
		);
		self.ck = Hash::from_bytes(out0);
		let mut temp_k: [u8; 32] = EMPTY_KEY;
		temp_k.copy_from_slice(&out1[..32]);
		self.cs = CipherState::from_key(Key::from_bytes(temp_k));
	}

	pub(crate) fn mix_hash(&mut self, data: &[u8]) {
		self.h = Hash::from_bytes(hash_with_context(&self.h.as_bytes()[..], data));
	}

	#[allow(dead_code)]
	pub(crate) fn mix_key_and_hash(&mut self, input_key_material: &[u8]) {
		let mut out0: [u8; HASHLEN] = EMPTY_HASH;
		let mut out1: [u8; HASHLEN] = EMPTY_HASH;
		let mut out2: [u8; HASHLEN] = EMPTY_HASH;
		hkdf(
			&self.ck.as_bytes()[..],
			input_key_material,
			3,
			&mut out0[..],
			&mut out1[..],
			&mut out2[..],
		);
		self.ck = Hash::from_bytes(out0);
		let temp_h: [u8; HASHLEN] = out1;
		let mut temp_k: [u8; DHLEN] = out2;
		self.mix_hash(&temp_h[..]);
		temp_k.copy_from_slice(&out2[..32]);
		self.cs = CipherState::from_key(Key::from_bytes(temp_k));
		out0.copy_from_slice(&EMPTY_HASH[..]);
		out1.copy_from_slice(&EMPTY_HASH[..]);
		out2.copy_from_slice(&EMPTY_HASH[..]);
	}

	#[allow(dead_code)]
	pub(crate) fn get_handshake_hash(&self) -> [u8; HASHLEN] {
		from_slice_hashlen(&self.h.as_bytes()[..])
	}

	pub(crate) fn encrypt_and_hash(&mut self, in_out: &mut [u8]) -> Result<(), NoiseError> {
			let mut temp_mac: [u8; MAC_LENGTH] = [0u8;MAC_LENGTH];
			let (plaintext, mac) = in_out.split_at_mut(in_out.len()-MAC_LENGTH);
			self.cs.encrypt_with_ad(&self.h.as_bytes()[..], plaintext, &mut temp_mac)?;
			mac.copy_from_slice(&temp_mac[..]);
			self.mix_hash(in_out);
			Ok(())
	}

	pub(crate) fn decrypt_and_hash(&mut self, in_out: &mut [u8]) -> Result<(), NoiseError> {
			let mut temp: [u8; 2048] = [0u8; 2048];
			temp[..in_out.len()].copy_from_slice(in_out);
			let (ciphertext, mac) = in_out.split_at_mut(in_out.len()-MAC_LENGTH);
			let mut temp_mac: [u8; MAC_LENGTH] = [0u8;MAC_LENGTH];
			temp_mac.copy_from_slice(mac);
			self.cs.decrypt_with_ad(&self.h.as_bytes()[..], ciphertext, &mut temp_mac)?;
			self.mix_hash(&temp[..in_out.len()]);
			Ok(())
	}

	pub(crate) fn split(&mut self) -> (CipherState, CipherState) {
		let mut temp_k1: [u8; HASHLEN] = EMPTY_HASH;
		let mut temp_k2: [u8; HASHLEN] = EMPTY_HASH;
		let mut out2: [u8; HASHLEN] = EMPTY_HASH;
		hkdf(
			&self.ck.as_bytes()[..],
			&ZEROLEN[..],
			2,
			&mut temp_k1[..],
			&mut temp_k2[..],
			&mut out2[..],
		);
		let cs1: CipherState =
			CipherState::from_key(Key::from_bytes(from_slice_hashlen(&temp_k1[..32])));
		temp_k1.copy_from_slice(&EMPTY_HASH[..]);
		let cs2: CipherState =
			CipherState::from_key(Key::from_bytes(from_slice_hashlen(&temp_k2[..32])));
		temp_k2.copy_from_slice(&EMPTY_HASH[..]);
		(cs1, cs2)
	}
}

#[derive(Clone)]
pub struct HandshakeState {
	ss:  SymmetricState,
	s:   Keypair,
	e:   Keypair,
	rs:  PublicKey,
	re:  PublicKey,
	psk: Psk,
}

/* HandshakeState */
impl HandshakeState {
	pub(crate) fn clear(&mut self) {
		self.s.clear();
		self.e.clear();
		self.re.clear();
		self.psk.clear();
	}

	pub fn get_remote_static_public_key(&self) -> PublicKey {
		self.rs
	}

	pub(crate) fn set_ephemeral_keypair(&mut self, e: Keypair) {
		self.e = e;
	}

	pub(crate) fn initialize_initiator(prologue: &[u8], s: Keypair, psk: Psk) -> HandshakeState {
		let protocol_name = b"Noise_XX_25519_ChaChaPoly_BLAKE2s";
		let mut ss: SymmetricState = SymmetricState::initialize_symmetric(&protocol_name[..]);
		ss.mix_hash(prologue);
		let rs = PublicKey::empty();
		HandshakeState{ss, s, e: Keypair::new_empty(), rs, re: PublicKey::empty(), psk}
	}

	pub(crate) fn initialize_responder(prologue: &[u8], s: Keypair, psk: Psk) -> HandshakeState {
		let protocol_name = b"Noise_XX_25519_ChaChaPoly_BLAKE2s";
		let mut ss: SymmetricState = SymmetricState::initialize_symmetric(&protocol_name[..]);
		ss.mix_hash(prologue);
		let rs = PublicKey::empty();
		HandshakeState{ss, s, e: Keypair::new_empty(), rs, re: PublicKey::empty(), psk}
	}
	pub(crate) fn write_message_a(&mut self, in_out: &mut [u8]) -> Result<(), NoiseError> {
		if in_out.len() < DHLEN {
			return Err(NoiseError::MissingneError);
		}
		if self.e.is_empty() {
			self.e = Keypair::new();
		}
		let (ne, in_out) = in_out.split_at_mut(DHLEN);
		ne.copy_from_slice(&self.e.get_public_key().as_bytes()[..]);
		self.ss.mix_hash(ne);
		/* No PSK, so skipping mixKey */
		self.ss.mix_hash(in_out);
		return Ok(());
	}

	pub(crate) fn write_message_b(&mut self, in_out: &mut [u8]) -> Result<(), NoiseError> {
		if in_out.len() < DHLEN {
			return Err(NoiseError::MissingneError);
		}
		if self.e.is_empty() {
			self.e = Keypair::new();
		}
		let (ne, in_out) = in_out.split_at_mut(DHLEN);
		ne.copy_from_slice(&self.e.get_public_key().as_bytes()[..]);
		self.ss.mix_hash(ne);
		/* No PSK, so skipping mixKey */
		self.ss.mix_key(&self.e.dh(&self.re.as_bytes()));
		//if in_out.len() < DHLEN {
		//	return Err(NoiseError::MissingnsError);
		//}
		let (ns, in_out) = in_out.split_at_mut(DHLEN+MAC_LENGTH);
		ns[..DHLEN].copy_from_slice(&self.s.get_public_key().as_bytes()[..]);
		self.ss.encrypt_and_hash(ns)?;
		self.ss.mix_key(&self.s.dh(&self.re.as_bytes()));
		self.ss.encrypt_and_hash(in_out)?;
		return Ok(());
	}

	pub(crate) fn write_message_c(&mut self, in_out: &mut [u8]) -> Result<(Hash, CipherState, CipherState), NoiseError> {
		//if in_out.len() < DHLEN {
		//	return Err(NoiseError::MissingnsError);
		//}
		let (ns, in_out) = in_out.split_at_mut(DHLEN+MAC_LENGTH);
		ns[..DHLEN].copy_from_slice(&self.s.get_public_key().as_bytes()[..]);
		self.ss.encrypt_and_hash(ns)?;
		self.ss.mix_key(&self.s.dh(&self.re.as_bytes()));
		self.ss.encrypt_and_hash(in_out)?;
		let h: Hash = Hash::from_bytes(from_slice_hashlen(&self.ss.h.as_bytes()));
		let (cs1, cs2) = self.ss.split();
		self.ss.clear();
		Ok((h, cs1, cs2))
	}


	pub(crate) fn read_message_a(&mut self, in_out: &mut [u8]) -> Result<(), NoiseError> {
		if in_out.len() < MAC_LENGTH+DHLEN {
			//missing re
		return 	Err(NoiseError::MissingreError);
		}
		let (re, in_out) = in_out.split_at_mut(DHLEN);
		self.re = PublicKey::from_bytes(from_slice_hashlen(re))?;
		self.ss.mix_hash(&self.re.as_bytes()[..DHLEN]);
		/* No PSK, so skipping mixKey */
		self.ss.mix_hash(in_out);
		return Ok(());
	}

	pub(crate) fn read_message_b(&mut self, in_out: &mut [u8]) -> Result<(), NoiseError> {
		if in_out.len() < MAC_LENGTH+DHLEN {
			//missing re
		return 	Err(NoiseError::MissingreError);
		}
		let (re, in_out) = in_out.split_at_mut(DHLEN);
		self.re = PublicKey::from_bytes(from_slice_hashlen(re))?;
		self.ss.mix_hash(&self.re.as_bytes()[..DHLEN]);
		/* No PSK, so skipping mixKey */
		self.ss.mix_key(&self.e.dh(&self.re.as_bytes()));
		if in_out.len() < MAC_LENGTH+DHLEN {
			//missing rs
			return Err(NoiseError::MissingrsError);
		}
		let (rs, in_out) = in_out.split_at_mut(MAC_LENGTH+DHLEN);
		self.ss.decrypt_and_hash(rs)?;
		self.rs = PublicKey::from_bytes(from_slice_hashlen(rs))?;
		self.ss.mix_key(&self.e.dh(&self.rs.as_bytes()));
		self.ss.decrypt_and_hash(in_out)?;
		return Ok(());
	}

	pub(crate) fn read_message_c(&mut self, in_out: &mut [u8]) ->  Result<(Hash, CipherState, CipherState), NoiseError> {
		if in_out.len() < MAC_LENGTH+DHLEN {
			//missing rs
			return Err(NoiseError::MissingrsError);
		}
		let (rs, in_out) = in_out.split_at_mut(MAC_LENGTH+DHLEN);
		self.ss.decrypt_and_hash(rs)?;
		self.rs = PublicKey::from_bytes(from_slice_hashlen(rs))?;
		self.ss.mix_key(&self.e.dh(&self.rs.as_bytes()));
		self.ss.decrypt_and_hash(in_out)?;
		let h: Hash = Hash::from_bytes(from_slice_hashlen(&self.ss.h.as_bytes()));
		let (cs1, cs2) = self.ss.split();
		self.ss.clear();
		Ok((h, cs1, cs2))
	}


}
