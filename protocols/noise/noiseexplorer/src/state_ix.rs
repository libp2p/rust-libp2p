/* ---------------------------------------------------------------- *
 * STATE MANAGEMENT                                                 *
 * ---------------------------------------------------------------- */

use crate::{consts::{DHLEN, EMPTY_HASH, EMPTY_KEY, HASHLEN, NONCE_LENGTH, ZEROLEN},
            error::NoiseError,
            prims::{decrypt, encrypt, hash, hkdf},
            types::{Hash, Key, Keypair, Nonce, Psk, PublicKey}};
use hacl_star::chacha20poly1305;

pub(crate) fn from_slice_hashlen(bytes: &[u8],) -> [u8; HASHLEN] {
	let mut array = EMPTY_HASH;
	let bytes = &bytes[..array.len()];
	array.copy_from_slice(bytes,);
	array
}

#[derive(Clone)]
pub(crate) struct CipherState {
	k: Key,
	n: Nonce,
}

impl CipherState {
	pub(crate) fn new() -> Self {
		Self::from_key(Key::new(),)
	}

	pub(crate) fn clear_key(&mut self,) {
		self.k.clear();
	}

	pub(crate) fn clear(&mut self,) {
		self.k.clear();
		self.n = Nonce::new();
	}

	pub(crate) fn from_key(k: Key,) -> Self {
		let nonce: Nonce = Nonce::new();
		Self {
			k,
			n: nonce,
		}
	}

	pub(crate) fn has_key(&self,) -> bool {
		!self.k.is_empty()
	}

	#[allow(dead_code)]
	pub(crate) fn set_nonce(&mut self, n: Nonce,) {
		self.n = n;
	}

	#[allow(dead_code)]
	pub(crate) fn get_nonce(&self,) -> Nonce {
		self.n
	}

	pub(crate) fn encrypt_with_ad(
		&mut self, ad: &[u8], plaintext: &[u8],
	) -> Result<Vec<u8,>, NoiseError,> {
		let nonce = self.n.get_value()?;
		if !self.has_key() {
			Ok(Vec::from(plaintext,),)
		}
		else {
			let ciphertext: Vec<u8,> =
				encrypt(from_slice_hashlen(&self.k.as_bytes()[..],), nonce, ad, plaintext,);
			self.n.increment();
			Ok(ciphertext,)
		}
	}

	pub(crate) fn decrypt_with_ad(
		&mut self, ad: &[u8], ciphertext: &[u8],
	) -> Result<Vec<u8,>, NoiseError,> {
		let nonce = self.n.get_value()?;
		if !self.has_key() {
			Ok(Vec::from(ciphertext,),)
		}
		else if let Some(plaintext,) =
			decrypt(from_slice_hashlen(&self.k.as_bytes()[..],), nonce, ad, ciphertext,)
		{
			self.n.increment();
			Ok(plaintext,)
		}
		else {
			Err(NoiseError::DecryptionError,)
		}
	}

	#[allow(dead_code)]
	pub(crate) fn rekey(&mut self,) {
		let mut in_out = EMPTY_KEY;
		chacha20poly1305::key(&self.k.as_bytes(),).nonce(&[0xFFu8; NONCE_LENGTH],).encrypt(
			&ZEROLEN[..],
			&mut in_out[..],
			&mut [0u8; 16],
		);
		self.k.clear();
		self.k = Key::from_bytes(in_out,);
	}

	pub(crate) fn write_message_regular(
		&mut self, payload: &[u8],
	) -> Result<Vec<u8,>, NoiseError,> {
		let output = self.encrypt_with_ad(&ZEROLEN[..], payload,)?;
		Ok(output,)
	}

	pub(crate) fn read_message_regular(
		&mut self, message: &[u8],
	) -> Result<Vec<u8,>, NoiseError,> {
		let out = self.decrypt_with_ad(&ZEROLEN[..], message,)?;
		Ok(out,)
	}
}

#[derive(Clone)]
pub struct SymmetricState {
	cs: CipherState,
	ck: Hash,
	h:  Hash,
}

impl SymmetricState {
	pub(crate) fn clear(&mut self,) {
		self.cs.clear_key();
		self.ck.clear();
	}

	pub fn initialize_symmetric(protocol_name: &[u8],) -> Self {
		let h: Hash;
		match protocol_name.len() {
			0..=31 => {
				let mut temp = Vec::from(protocol_name,);
				while temp.len() != HASHLEN {
					temp.push(0u8,);
				}
				h = Hash::from_bytes(from_slice_hashlen(&temp[..],),);
			},
			32 => h = Hash::from_bytes(from_slice_hashlen(protocol_name,),),
			_ => h = Hash::from_bytes(hash(protocol_name,),),
		}
		let ck: Hash = Hash::from_bytes(from_slice_hashlen(&h.as_bytes()[..],),);
		let cs: CipherState = CipherState::new();
		Self {
			cs,
			ck,
			h,
		}
	}

	pub(crate) fn mix_key(&mut self, input_key_material: &[u8],) {
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
		self.ck = Hash::from_bytes(out0,);
		let mut temp_k: [u8; 32] = EMPTY_KEY;
		temp_k.copy_from_slice(&out1[..32],);
		self.cs = CipherState::from_key(Key::from_bytes(temp_k,),);
	}

	pub(crate) fn mix_hash(&mut self, data: &[u8],) {
		let mut temp: Vec<u8,> = Vec::from(&self.h.as_bytes()[..],);
		temp.extend(data,);
		self.h = Hash::from_bytes(hash(&temp[..],),);
	}

	#[allow(dead_code)]
	pub(crate) fn mix_key_and_hash(&mut self, input_key_material: &[u8],) {
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
		self.ck = Hash::from_bytes(out0,);
		let temp_h: [u8; HASHLEN] = out1;
		let mut temp_k: [u8; DHLEN] = out2;
		self.mix_hash(&temp_h[..],);
		temp_k.copy_from_slice(&out2[..32],);
		self.cs = CipherState::from_key(Key::from_bytes(temp_k,),);
	}

	#[allow(dead_code)]
	pub(crate) fn get_handshake_hash(&self,) -> [u8; HASHLEN] {
		from_slice_hashlen(&self.h.as_bytes()[..],)
	}

	pub(crate) fn encrypt_and_hash(&mut self, plaintext: &[u8],) -> Result<Vec<u8,>, NoiseError,> {
		let ciphertext: Vec<u8,> = self.cs.encrypt_with_ad(&self.h.as_bytes()[..], plaintext,)?;
		self.mix_hash(&ciphertext,);
		Ok(ciphertext,)
	}

	pub(crate) fn decrypt_and_hash(&mut self, ciphertext: &[u8],) -> Result<Vec<u8,>, NoiseError,> {
		let plaintext = self.cs.decrypt_with_ad(&self.h.as_bytes()[..], &ciphertext,)?;
		self.mix_hash(ciphertext,);
		Ok(plaintext,)
	}

	pub(crate) fn split(&mut self,) -> (CipherState, CipherState,) {
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
			CipherState::from_key(Key::from_bytes(from_slice_hashlen(&temp_k1[..32],),),);
		let cs2: CipherState =
			CipherState::from_key(Key::from_bytes(from_slice_hashlen(&temp_k2[..32],),),);
		(cs1, cs2,)
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
	pub(crate) fn clear(&mut self,) {
		self.s.clear();
		self.e.clear();
		self.rs.clear();
		self.re.clear();
		self.psk.clear();
	}

	pub(crate) fn set_ephemeral_keypair(&mut self, e: Keypair,) {
		self.e = e;
	}
	pub(crate) fn initialize_initiator(prologue: &[u8], s: Keypair, rs: PublicKey, psk: Psk) -> HandshakeState {
		let protocol_name = b"Noise_IX_25519_ChaChaPoly_BLAKE2s";
		let mut ss: SymmetricState = SymmetricState::initialize_symmetric(&protocol_name[..]);
		ss.mix_hash(prologue);
		HandshakeState{ss, s, e: Keypair::new_empty(), rs, re: PublicKey::empty(), psk}
	}

	pub(crate) fn initialize_responder(prologue: &[u8], s: Keypair, rs: PublicKey, psk: Psk) -> HandshakeState {
		let protocol_name = b"Noise_IX_25519_ChaChaPoly_BLAKE2s";
		let mut ss: SymmetricState = SymmetricState::initialize_symmetric(&protocol_name[..]);
		ss.mix_hash(prologue);
		HandshakeState{ss, s, e: Keypair::new_empty(), rs, re: PublicKey::empty(), psk}
	}
	pub(crate) fn write_message_a(&mut self, input: &[u8]) -> Result<Vec<u8>, NoiseError> {
		let mut output: Vec<u8> = Vec::new();
		if self.e.is_empty() {
			self.e = Keypair::new();
		}
		let ne = self.e.get_public_key().as_bytes();
		self.ss.mix_hash(&ne[..]);
		/* No PSK, so skipping mixKey */
		output.append(&mut Vec::from(&ne[..]));
		let mut ns = self.ss.encrypt_and_hash(&self.s.get_public_key().as_bytes()[..])?;
		output.append(&mut ns);
		let mut ciphertext = self.ss.encrypt_and_hash(input)?;
		output.append(&mut ciphertext);
		Ok(output)
	}

	pub(crate) fn write_message_b(&mut self, input: &[u8]) -> Result<(Hash, Vec<u8>, CipherState, CipherState), NoiseError> {
		let mut output: Vec<u8> = Vec::new();
		if self.e.is_empty() {
			self.e = Keypair::new();
		}
		let ne = self.e.get_public_key().as_bytes();
		self.ss.mix_hash(&ne[..]);
		/* No PSK, so skipping mixKey */
		output.append(&mut Vec::from(&ne[..]));
		self.ss.mix_key(&self.e.dh(&self.re.as_bytes()));
		self.ss.mix_key(&self.e.dh(&self.rs.as_bytes()));
		let mut ns = self.ss.encrypt_and_hash(&self.s.get_public_key().as_bytes()[..])?;
		output.append(&mut ns);
		self.ss.mix_key(&self.s.dh(&self.re.as_bytes()));
		let mut ciphertext = self.ss.encrypt_and_hash(input)?;
		output.append(&mut ciphertext);
		let h: Hash = Hash::from_bytes(from_slice_hashlen(&self.ss.h.as_bytes()));
		let (cs1, cs2) = self.ss.split();
		self.ss.clear();
		Ok((h, output, cs1, cs2))
	}


	pub(crate) fn read_message_a(&mut self, input: &[u8]) -> Result<Vec<u8>, NoiseError> {
		let rest = input;
		let (vre, rest) = rest.split_at(32);
		self.re = PublicKey::from_bytes(from_slice_hashlen(vre));
		self.ss.mix_hash(&self.re.as_bytes()[..DHLEN]);
		/* No PSK, so skipping mixKey */
		let (vrs, rest) = rest.split_at(32);
		let x = self.ss.decrypt_and_hash(vrs)?;
		self.rs = PublicKey::from_bytes(from_slice_hashlen(&x[..]));
		let output = self.ss.decrypt_and_hash(rest)?;
		Ok(output)
	}

	pub(crate) fn read_message_b(&mut self, input: &[u8]) ->  Result<(Hash, Vec<u8>, CipherState, CipherState), NoiseError> {
		let rest = input;
		let (vre, rest) = rest.split_at(32);
		self.re = PublicKey::from_bytes(from_slice_hashlen(vre));
		self.ss.mix_hash(&self.re.as_bytes()[..DHLEN]);
		/* No PSK, so skipping mixKey */
		self.ss.mix_key(&self.e.dh(&self.re.as_bytes()));
		self.ss.mix_key(&self.s.dh(&self.re.as_bytes()));
		let (vrs, rest) = rest.split_at(48);
		let x = self.ss.decrypt_and_hash(vrs)?;
		self.rs = PublicKey::from_bytes(from_slice_hashlen(&x[..]));
		self.ss.mix_key(&self.e.dh(&self.rs.as_bytes()));
		let output = self.ss.decrypt_and_hash(rest)?;
		let h: Hash = Hash::from_bytes(from_slice_hashlen(&self.ss.h.as_bytes()));
		let (cs1, cs2) = self.ss.split();
		self.ss.clear();
		Ok((h, output, cs1, cs2))
	}


}
