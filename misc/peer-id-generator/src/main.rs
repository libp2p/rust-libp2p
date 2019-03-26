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

use libp2p_core::identity;
use std::{env, str, thread, time::Duration};

fn main() {
    // Due to the fact that a peer id uses a SHA-256 multihash, it always starts with the
    // bytes 0x1220, meaning that only some characters are valid.
    const ALLOWED_FIRST_BYTE: &'static [u8] = b"NPQRSTUVWXYZ";

    let prefix =
        match env::args().nth(1) {
            Some(prefix) => prefix,
            None => {
                println!(
                "Usage: {} <prefix>\n\n\
                 Generates a peer id that starts with the chosen prefix using a secp256k1 public \
                 key.\n\n\
                 Prefix must be a sequence of characters in the base58 \
                 alphabet, and must start with one of the following: {}",
                env::current_exe().unwrap().file_name().unwrap().to_str().unwrap(),
                str::from_utf8(ALLOWED_FIRST_BYTE).unwrap()
            );
                return;
            }
        };

    // The base58 alphabet is not necessarily obvious.
    const ALPHABET: &'static [u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    if prefix.as_bytes().iter().any(|c| !ALPHABET.contains(c)) {
        println!("Prefix {} is not valid base58", prefix);
        return;
    }

    // Checking conformity to ALLOWED_FIRST_BYTE.
    if !prefix.is_empty() {
        if !ALLOWED_FIRST_BYTE.contains(&prefix.as_bytes()[0]) {
            println!("Prefix {} is not reachable", prefix);
            println!(
                "Only the following bytes are possible as first byte: {}",
                str::from_utf8(ALLOWED_FIRST_BYTE).unwrap()
            );
            return;
        }
    }

    // Find peer IDs in a multithreaded fashion.
    for _ in 0..num_cpus::get() {
        let prefix = prefix.clone();
        thread::spawn(move || loop {
            let keypair = identity::ed25519::Keypair::generate();
            let secret = keypair.secret();
            let peer_id = identity::PublicKey::Ed25519(keypair.public()).into_peer_id();
            let base58 = peer_id.to_base58();
            if base58[2..].starts_with(&prefix) {
                println!("Found {:?}", peer_id);
                println!("=> Private key = {:?}", secret.as_ref());
            }
        });
    }

    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}
