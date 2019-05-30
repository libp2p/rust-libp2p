#![allow(non_snake_case, non_upper_case_globals, unused_assignments, unused_imports)]

use noiseexplorer::{
	consts::{DHLEN, MAC_LENGTH},
	error::NoiseError,
	noisesession_ik, noisesession_ix, noisesession_xx,
	types::{Keypair, PrivateKey, PublicKey},
};

fn decode_str(s: &str) -> Vec<u8> {
 	hex::decode(s).unwrap()
 }

#[test]
fn noiseexplorer_test_ik() {

	let mut prologue: Vec<u8> = Vec::new();
	let mut messageA: Vec<u8> = Vec::new();
	let mut messageB: Vec<u8> = Vec::new();
	let mut messageC: Vec<u8> = Vec::new();
	let mut messageD: Vec<u8> = Vec::new();
	let mut messageE: Vec<u8> = Vec::new();
	let mut messageF: Vec<u8> = Vec::new();

	prologue = decode_str("4a6f686e2047616c74");
	let initiator_static_private = PrivateKey::from_str("e61ef9919cde45dd5f82166404bd08e38bceb5dfdfded0a34c8df7ed542214d1").unwrap();
	let responder_static_private = PrivateKey::from_str("4a3acbfdb163dec651dfa3194dece676d437029c62a408b4c5ea9114246e4893").unwrap();
	let responder_static_public = responder_static_private.generate_public_key().unwrap();
	let initiator_static_kp = Keypair::from_private_key(initiator_static_private).unwrap();
	let responder_static_kp = Keypair::from_private_key(responder_static_private).unwrap();

	let mut initiator_session: noisesession_ik::NoiseSession = noisesession_ik::NoiseSession::init_session(true, &prologue[..], initiator_static_kp, Some(responder_static_public));
	let mut responder_session: noisesession_ik::NoiseSession = noisesession_ik::NoiseSession::init_session(false, &prologue[..], responder_static_kp, None);
	let initiator_ephemeral_private = PrivateKey::from_str("893e28b9dc6ca8d611ab664754b8ceb7bac5117349a4439a6b0569da977c464a").unwrap();
	let initiator_ephemeral_kp = Keypair::from_private_key(initiator_ephemeral_private).unwrap();
	initiator_session.set_ephemeral_keypair(initiator_ephemeral_kp);
	let responder_ephemeral_private = PrivateKey::from_str("bbdb4cdbd309f1a1f2e1456967fe288cadd6f712d65dc7b7793d5e63da6b375b").unwrap();
	let responder_ephemeral_kp = Keypair::from_private_key(responder_ephemeral_private).unwrap();
	responder_session.set_ephemeral_keypair(responder_ephemeral_kp);
	messageA.extend_from_slice(&[0u8; DHLEN][..]);
	messageA.extend_from_slice(&[0u8; DHLEN+MAC_LENGTH][..]);
	messageA.extend_from_slice(&decode_str("4c756477696720766f6e204d69736573")[..]);
	messageA.extend_from_slice(&[0u8; MAC_LENGTH][..]);
	let tA: Vec<u8> = Vec::from(&decode_str("ca35def5ae56cec33dc2036731ab14896bc4c75dbb07a61f879f8e3afa4c79440b03ddc7aac5123d06a1b23b71670e32e76c28239a7ca4ac8f784de7e44c1adbfc6e83fef7352a58d9d56157400c0a737b1d171ce368229c7b752ac25b8faf4eca690f6d896f543be02c996ab2b86b76")[..]);
	// messageA length is 96 + payload length,
	// payload starts at index 80
	initiator_session.send_message(&mut messageA[..]).unwrap();
	responder_session.recv_message(&mut messageA.clone()[..]).unwrap();
	messageB.extend_from_slice(&[0u8; DHLEN][..]);
	messageB.extend_from_slice(&decode_str("4d757272617920526f746862617264")[..]);
	messageB.extend_from_slice(&[0u8; MAC_LENGTH][..]);
	let tB: Vec<u8> = Vec::from(&decode_str("95ebc60d2b1fa672c1f46a8aa265ef51bfe38e7ccb39ec5be34069f144808843d9b5a8927f0ac9655ef76833bc7e5561f42e691ac8404efd6fbd6308b6a27c")[..]);
	// messageB length is 48 + payload length,
	// payload starts at index 32
	responder_session.send_message(&mut messageB[..]).unwrap();
	initiator_session.recv_message(&mut messageB.clone()[..]).unwrap();
	messageC.extend_from_slice(&decode_str("462e20412e20486179656b")[..]);
	messageC.extend_from_slice(&[0u8; MAC_LENGTH][..]);
	let tC: Vec<u8> = Vec::from(&decode_str("2c256ed08fcd08c2980f954ee4beaccb61c9581340f5dd2fd1cf3b")[..]);
	// messageC length is 16 + payload length,
	// payload starts at index 0
	initiator_session.send_message(&mut messageC[..]).unwrap();
	responder_session.recv_message(&mut messageC.clone()[..]).unwrap();
	messageD.extend_from_slice(&decode_str("4361726c204d656e676572")[..]);
	messageD.extend_from_slice(&[0u8; MAC_LENGTH][..]);
	let tD: Vec<u8> = Vec::from(&decode_str("d6033f70eee20945c7c9dba304e397ee3b284ff5e00fd9efb095d3")[..]);
	// messageD length is 16 + payload length,
	// payload starts at index 0
	responder_session.send_message(&mut messageD[..]).unwrap();
	initiator_session.recv_message(&mut messageD.clone()[..]).unwrap();
	messageE.extend_from_slice(&decode_str("4a65616e2d426170746973746520536179")[..]);
	messageE.extend_from_slice(&[0u8; MAC_LENGTH][..]);
	let tE: Vec<u8> = Vec::from(&decode_str("a9c068ca5d8babf72560652d8e851adbfac35c8a66e810d560863173e96adf4cfe")[..]);
	// messageE length is 16 + payload length,
	// payload starts at index 0
	initiator_session.send_message(&mut messageE[..]).unwrap();
	responder_session.recv_message(&mut messageE.clone()[..]).unwrap();
	messageF.extend_from_slice(&decode_str("457567656e2042f6686d20766f6e2042617765726b")[..]);
	messageF.extend_from_slice(&[0u8; MAC_LENGTH][..]);
	let tF: Vec<u8> = Vec::from(&decode_str("2a09d8f459e5927e40fdd2eddc99bdafb04e13a26f145cb5cfe9e6ba34c94331ebc17d5156")[..]);
	// messageF length is 16 + payload length,
	// payload starts at index 0
	responder_session.send_message(&mut messageF[..]).unwrap();
	initiator_session.recv_message(&mut messageF.clone()[..]).unwrap();
	assert!(tA == messageA, "\n\n\nTest A: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tA, messageA);
	assert!(tB == messageB, "\n\n\nTest B: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tB, messageB);
	assert!(tC == messageC, "\n\n\nTest C: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tC, messageC);
	assert!(tD == messageD, "\n\n\nTest D: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tD, messageD);
	assert!(tE == messageE, "\n\n\nTest E: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tE, messageE);
	assert!(tF == messageF, "\n\n\nTest F: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tF, messageF);
}

#[test]
fn noiseexplorer_test_ix() {

	let mut prologue: Vec<u8> = Vec::new();
	let mut messageA: Vec<u8> = Vec::new();
	let mut messageB: Vec<u8> = Vec::new();
	let mut messageC: Vec<u8> = Vec::new();
	let mut messageD: Vec<u8> = Vec::new();
	let mut messageE: Vec<u8> = Vec::new();
	let mut messageF: Vec<u8> = Vec::new();

	prologue = decode_str("4a6f686e2047616c74");
	let initiator_static_private = PrivateKey::from_str("e61ef9919cde45dd5f82166404bd08e38bceb5dfdfded0a34c8df7ed542214d1").unwrap();
	let responder_static_private = PrivateKey::from_str("4a3acbfdb163dec651dfa3194dece676d437029c62a408b4c5ea9114246e4893").unwrap();
	let initiator_static_kp = Keypair::from_private_key(initiator_static_private).unwrap();
	let responder_static_kp = Keypair::from_private_key(responder_static_private).unwrap();

	let mut initiator_session: noisesession_ix::NoiseSession = noisesession_ix::NoiseSession::init_session(true, &prologue[..], initiator_static_kp);
	let mut responder_session: noisesession_ix::NoiseSession = noisesession_ix::NoiseSession::init_session(false, &prologue[..], responder_static_kp);
	let initiator_ephemeral_private = PrivateKey::from_str("893e28b9dc6ca8d611ab664754b8ceb7bac5117349a4439a6b0569da977c464a").unwrap();
	let initiator_ephemeral_kp = Keypair::from_private_key(initiator_ephemeral_private).unwrap();
	initiator_session.set_ephemeral_keypair(initiator_ephemeral_kp);
	let responder_ephemeral_private = PrivateKey::from_str("bbdb4cdbd309f1a1f2e1456967fe288cadd6f712d65dc7b7793d5e63da6b375b").unwrap();
	let responder_ephemeral_kp = Keypair::from_private_key(responder_ephemeral_private).unwrap();
	responder_session.set_ephemeral_keypair(responder_ephemeral_kp);
	messageA.extend_from_slice(&[0u8; DHLEN][..]);
	messageA.extend_from_slice(&[0u8; DHLEN][..]);
	messageA.extend_from_slice(&decode_str("4c756477696720766f6e204d69736573")[..]);
	let tA: Vec<u8> = Vec::from(&decode_str("ca35def5ae56cec33dc2036731ab14896bc4c75dbb07a61f879f8e3afa4c79446bc3822a2aa7f4e6981d6538692b3cdf3e6df9eea6ed269eb41d93c22757b75a4c756477696720766f6e204d69736573")[..]);
	// messageA length is 64 + payload length,
	// payload starts at index 64
	initiator_session.send_message(&mut messageA[..]).unwrap();
	responder_session.recv_message(&mut messageA.clone()[..]).unwrap();
	messageB.extend_from_slice(&[0u8; DHLEN][..]);
	messageB.extend_from_slice(&[0u8; DHLEN+MAC_LENGTH][..]);
	messageB.extend_from_slice(&decode_str("4d757272617920526f746862617264")[..]);
	messageB.extend_from_slice(&[0u8; MAC_LENGTH][..]);
	let tB: Vec<u8> = Vec::from(&decode_str("95ebc60d2b1fa672c1f46a8aa265ef51bfe38e7ccb39ec5be34069f14480884398e7f90d906b0948dbc71ea7020ce711a6cfde5ed7ad1d43def67fb5be6190b5028fbb2556e9378b65b5e86195a7cd4cadddad64de91fbd1aaaae8621d31358a73dbfd6b68b96fb5bb8972bc28c2e2")[..]);
	// messageB length is 96 + payload length,
	// payload starts at index 80
	responder_session.send_message(&mut messageB[..]).unwrap();
	initiator_session.recv_message(&mut messageB.clone()[..]).unwrap();
	messageC.extend_from_slice(&decode_str("462e20412e20486179656b")[..]);
	messageC.extend_from_slice(&[0u8; MAC_LENGTH][..]);
	let tC: Vec<u8> = Vec::from(&decode_str("62bc36955e7d6399c18531eb05fc8f4646da466a98a7e5cf1942e7")[..]);
	// messageC length is 16 + payload length,
	// payload starts at index 0
	initiator_session.send_message(&mut messageC[..]).unwrap();
	responder_session.recv_message(&mut messageC.clone()[..]).unwrap();
	messageD.extend_from_slice(&decode_str("4361726c204d656e676572")[..]);
	messageD.extend_from_slice(&[0u8; MAC_LENGTH][..]);
	let tD: Vec<u8> = Vec::from(&decode_str("6be3ee3f7e5ccc4152754e4b22d87ee0045e6cd84654fd2ceb3720")[..]);
	// messageD length is 16 + payload length,
	// payload starts at index 0
	responder_session.send_message(&mut messageD[..]).unwrap();
	initiator_session.recv_message(&mut messageD.clone()[..]).unwrap();
	messageE.extend_from_slice(&decode_str("4a65616e2d426170746973746520536179")[..]);
	messageE.extend_from_slice(&[0u8; MAC_LENGTH][..]);
	let tE: Vec<u8> = Vec::from(&decode_str("19b242089e28f5b8c2881f36dacb6953de1b576b722359a0ab8ac478c3c8fcacb1")[..]);
	// messageE length is 16 + payload length,
	// payload starts at index 0
	initiator_session.send_message(&mut messageE[..]).unwrap();
	responder_session.recv_message(&mut messageE.clone()[..]).unwrap();
	messageF.extend_from_slice(&decode_str("457567656e2042f6686d20766f6e2042617765726b")[..]);
	messageF.extend_from_slice(&[0u8; MAC_LENGTH][..]);
	let tF: Vec<u8> = Vec::from(&decode_str("8db09f596ff2651900ff82316220328bb0ac49a520c58ff2504c67bb02c550d9546c483708")[..]);
	// messageF length is 16 + payload length,
	// payload starts at index 0
	responder_session.send_message(&mut messageF[..]).unwrap();
	initiator_session.recv_message(&mut messageF.clone()[..]).unwrap();
	assert!(tA == messageA, "\n\n\nTest A: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tA, messageA);
	assert!(tB == messageB, "\n\n\nTest B: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tB, messageB);
	assert!(tC == messageC, "\n\n\nTest C: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tC, messageC);
	assert!(tD == messageD, "\n\n\nTest D: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tD, messageD);
	assert!(tE == messageE, "\n\n\nTest E: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tE, messageE);
	assert!(tF == messageF, "\n\n\nTest F: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tF, messageF);
}

#[test]
fn noiseexplorer_test_xx() {

	let mut prologue: Vec<u8> = Vec::new();
	let mut messageA: Vec<u8> = Vec::new();
	let mut messageB: Vec<u8> = Vec::new();
	let mut messageC: Vec<u8> = Vec::new();
	let mut messageD: Vec<u8> = Vec::new();
	let mut messageE: Vec<u8> = Vec::new();
	let mut messageF: Vec<u8> = Vec::new();

	prologue = decode_str("4a6f686e2047616c74");
	let initiator_static_private = PrivateKey::from_str("e61ef9919cde45dd5f82166404bd08e38bceb5dfdfded0a34c8df7ed542214d1").unwrap();
	let responder_static_private = PrivateKey::from_str("4a3acbfdb163dec651dfa3194dece676d437029c62a408b4c5ea9114246e4893").unwrap();
	let initiator_static_kp = Keypair::from_private_key(initiator_static_private).unwrap();
	let responder_static_kp = Keypair::from_private_key(responder_static_private).unwrap();

	let mut initiator_session: noisesession_xx::NoiseSession = noisesession_xx::NoiseSession::init_session(true, &prologue[..], initiator_static_kp);
	let mut responder_session: noisesession_xx::NoiseSession = noisesession_xx::NoiseSession::init_session(false, &prologue[..], responder_static_kp);
	let initiator_ephemeral_private = PrivateKey::from_str("893e28b9dc6ca8d611ab664754b8ceb7bac5117349a4439a6b0569da977c464a").unwrap();
	let initiator_ephemeral_kp = Keypair::from_private_key(initiator_ephemeral_private).unwrap();
	initiator_session.set_ephemeral_keypair(initiator_ephemeral_kp);
	let responder_ephemeral_private = PrivateKey::from_str("bbdb4cdbd309f1a1f2e1456967fe288cadd6f712d65dc7b7793d5e63da6b375b").unwrap();
	let responder_ephemeral_kp = Keypair::from_private_key(responder_ephemeral_private).unwrap();
	responder_session.set_ephemeral_keypair(responder_ephemeral_kp);
	messageA.extend_from_slice(&[0u8; DHLEN][..]);
	messageA.extend_from_slice(&decode_str("4c756477696720766f6e204d69736573")[..]);
	let tA: Vec<u8> = Vec::from(&decode_str("ca35def5ae56cec33dc2036731ab14896bc4c75dbb07a61f879f8e3afa4c79444c756477696720766f6e204d69736573")[..]);
	// messageA length is 32 + payload length,
	// payload starts at index 32
	initiator_session.send_message(&mut messageA[..]).unwrap();
	responder_session.recv_message(&mut messageA.clone()[..]).unwrap();
	messageB.extend_from_slice(&[0u8; DHLEN][..]);
	messageB.extend_from_slice(&[0u8; DHLEN+MAC_LENGTH][..]);
	messageB.extend_from_slice(&decode_str("4d757272617920526f746862617264")[..]);
	messageB.extend_from_slice(&[0u8; MAC_LENGTH][..]);
	let tB: Vec<u8> = Vec::from(&decode_str("95ebc60d2b1fa672c1f46a8aa265ef51bfe38e7ccb39ec5be34069f1448088437c365eb362a1c991b0557fe8a7fb187d99346765d93ec63db6c1b01504ebeec55a2298d2dbff80eff034d20595153f63a196a6cead1e11b2bb13e336fa13616dd3e8b0a070c882ed3f1a78c7c06c93")[..]);
	// messageB length is 96 + payload length,
	// payload starts at index 80
	responder_session.send_message(&mut messageB[..]).unwrap();
	initiator_session.recv_message(&mut messageB.clone()[..]).unwrap();
	messageC.extend_from_slice(&[0u8; DHLEN+MAC_LENGTH][..]);
	messageC.extend_from_slice(&decode_str("462e20412e20486179656b")[..]);
	messageC.extend_from_slice(&[0u8; MAC_LENGTH][..]);
	let tC: Vec<u8> = Vec::from(&decode_str("46c3307de83b014258717d97781c1f50936d8b7d50c0722a1739654d10392d415b670c114f79b9a4f80541570f77ce88802efa4220cff733e7b5668ba38059ec904b4b8eef9448085faf51")[..]);
	// messageC length is 64 + payload length,
	// payload starts at index 48
	initiator_session.send_message(&mut messageC[..]).unwrap();
	responder_session.recv_message(&mut messageC.clone()[..]).unwrap();
	messageD.extend_from_slice(&decode_str("4361726c204d656e676572")[..]);
	messageD.extend_from_slice(&[0u8; MAC_LENGTH][..]);
	let tD: Vec<u8> = Vec::from(&decode_str("d5e83adfaac5dc324a68f1862df54549e56d209fba707205f328b2")[..]);
	// messageD length is 16 + payload length,
	// payload starts at index 0
	responder_session.send_message(&mut messageD[..]).unwrap();
	initiator_session.recv_message(&mut messageD.clone()[..]).unwrap();
	messageE.extend_from_slice(&decode_str("4a65616e2d426170746973746520536179")[..]);
	messageE.extend_from_slice(&[0u8; MAC_LENGTH][..]);
	let tE: Vec<u8> = Vec::from(&decode_str("d102c9029b1f55c788f561ba7737afbccef9c9f1bf2f238167fd40ba9c1c134867")[..]);
	// messageE length is 16 + payload length,
	// payload starts at index 0
	initiator_session.send_message(&mut messageE[..]).unwrap();
	responder_session.recv_message(&mut messageE.clone()[..]).unwrap();
	messageF.extend_from_slice(&decode_str("457567656e2042f6686d20766f6e2042617765726b")[..]);
	messageF.extend_from_slice(&[0u8; MAC_LENGTH][..]);
	let tF: Vec<u8> = Vec::from(&decode_str("cb1ce80960382c6d5d5e740ffb724d1432f0310b200fb6f8424120f506092744baa415e155")[..]);
	// messageF length is 16 + payload length,
	// payload starts at index 0
	responder_session.send_message(&mut messageF[..]).unwrap();
	initiator_session.recv_message(&mut messageF.clone()[..]).unwrap();
	assert!(tA == messageA, "\n\n\nTest A: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tA, messageA);
	assert!(tB == messageB, "\n\n\nTest B: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tB, messageB);
	assert!(tC == messageC, "\n\n\nTest C: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tC, messageC);
	assert!(tD == messageD, "\n\n\nTest D: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tD, messageD);
	assert!(tE == messageE, "\n\n\nTest E: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tE, messageE);
	assert!(tF == messageF, "\n\n\nTest F: FAIL\n\nExpected:\n{:X?}\n\nActual:\n{:X?}", tF, messageF);
}
