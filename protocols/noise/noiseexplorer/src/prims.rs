/* ---------------------------------------------------------------- *
 * PRIMITIVES                                                       *
 * ---------------------------------------------------------------- */

use crate::{consts::{BLOCKLEN, DHLEN, EMPTY_HASH, HASHLEN, MAC_LENGTH, NONCE_LENGTH},
            state_xx::from_slice_hashlen,
            types::Keypair};
use blake2_rfc::blake2s::Blake2s;
use hacl_star::chacha20poly1305;

#[allow(dead_code)]
pub fn generate_keypair() -> Keypair {
	Keypair::new()
}

pub(crate) fn encrypt(k: [u8; DHLEN], n: u64, ad: &[u8], plaintext: &[u8],) -> Vec<u8,> {
	let mut mac: [u8; MAC_LENGTH] = [0_u8; MAC_LENGTH];
	let mut in_out = plaintext.to_owned();
	let mut nonce: [u8; NONCE_LENGTH] = [0_u8; NONCE_LENGTH];
	nonce[4..].copy_from_slice(&n.to_le_bytes(),);
	chacha20poly1305::key(&k,).nonce(&nonce,).encrypt(ad, &mut in_out[..], &mut mac,);
	let mut ciphertext: Vec<u8,> = in_out;
	ciphertext.extend_from_slice(&mac[..],);
	ciphertext
}

pub(crate) fn decrypt(k: [u8; DHLEN], n: u64, ad: &[u8], ciphertext: &[u8],) -> Option<Vec<u8,>,> {
	let (x, y,) = ciphertext.split_at(ciphertext.len() - MAC_LENGTH,);
	let mut in_out = x.to_owned();
	let mut mac: [u8; MAC_LENGTH] = [0_u8; MAC_LENGTH];
	mac.copy_from_slice(y,);
	let mut nonce: [u8; NONCE_LENGTH] = [0_u8; NONCE_LENGTH];
	nonce[4..].copy_from_slice(&n.to_le_bytes(),);
	let decryption_status =
		chacha20poly1305::key(&k,).nonce(&nonce,).decrypt(ad, &mut in_out[..], &mac,);
	if decryption_status {
		Some(in_out,)
	}
	else {
		None
	}
}

pub(crate) fn hash(data: &[u8],) -> [u8; HASHLEN] {
	let mut context = Blake2s::new(HASHLEN,);
	context.update(data,);
	let hash = context.finalize();
	from_slice_hashlen(&hash.as_bytes()[..],)
}

pub(crate) fn hmac(key: &[u8], data: &[u8], out: &mut [u8],) {
	let mut context = Blake2s::new(HASHLEN,);
	let mut ipad = [0x36_u8; BLOCKLEN];
	let mut opad = [0x5c_u8; BLOCKLEN];
	for count in 0..key.len() {
		ipad[count] ^= key[count];
		opad[count] ^= key[count];
	}
	context.update(&ipad[..BLOCKLEN],);
	context.update(data,);
	let inner_output = context.finalize();
	context = Blake2s::new(HASHLEN,);
	context.update(&opad[..BLOCKLEN],);
	context.update(&inner_output.as_bytes()[..HASHLEN],);
	out.copy_from_slice(context.finalize().as_bytes(),);
}

pub(crate) fn hkdf(
	chaining_key: &[u8], input_key_material: &[u8], outputs: usize, out1: &mut [u8],
	out2: &mut [u8], out3: &mut [u8],
) {
	let mut temp_key = EMPTY_HASH;
	hmac(chaining_key, input_key_material, &mut temp_key,);
	hmac(&temp_key, &[1u8,], out1,);
	if outputs == 1 {
		return;
	}
	let mut in2 = [0_u8; HASHLEN + 1];
	copy_slices!(&out1[0..HASHLEN], &mut in2);
	in2[HASHLEN] = 2;
	hmac(&temp_key, &in2[..=HASHLEN], out2,);
	if outputs == 2 {
		return;
	}
	let mut in3 = [0_u8; HASHLEN + 1];
	copy_slices!(&out2[0..HASHLEN], &mut in3);
	in3[HASHLEN] = 3;
	hmac(&temp_key, &in3[..=HASHLEN], out3,);
}
