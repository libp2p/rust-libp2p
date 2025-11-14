// generate-js-fixture.js
import { generateKeyPair } from '@libp2p/crypto/keys';
import { peerIdFromPrivateKey } from '@libp2p/peer-id';
import { PeerRecord, RecordEnvelope } from '@libp2p/peer-record';
import { multiaddr } from '@multiformats/multiaddr';
import fs from 'fs';

const privateKey = await generateKeyPair('Ed25519');
const peerId = peerIdFromPrivateKey(privateKey);

const record = new PeerRecord({
    peerId,
    multiaddrs: [multiaddr('/ip4/127.0.0.1/tcp/4001')]
});

const envelope = await RecordEnvelope.seal(record, privateKey);
const wireData = envelope.marshal();

// Save to file
fs.writeFileSync('js-signed-peer-record.bin', wireData);

console.log('Generated js-signed-peer-record.bin');
console.log('PeerID:', peerId.toString());
console.log('Domain:', PeerRecord.DOMAIN);
console.log('Payload Type:', Array.from(PeerRecord.CODEC));