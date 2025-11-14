package main

import (
    "fmt"
    "os"
    
    "github.com/libp2p/go-libp2p/core/crypto"
    "github.com/libp2p/go-libp2p/core/peer"
    "github.com/libp2p/go-libp2p/core/record"
    ma "github.com/multiformats/go-multiaddr"
)

func main() {
    // Generate key
    priv, _, err := crypto.GenerateKeyPair(crypto.Ed25519, -1)
    if err != nil {
        panic(err)
    }
    
    pid, err := peer.IDFromPrivateKey(priv)
    if err != nil {
        panic(err)
    }
    
    // Create multiaddr
    addr, _ := ma.NewMultiaddr("/ip4/127.0.0.1/tcp/4001")
    
    // Create peer record
    rec := &peer.PeerRecord{
        PeerID: pid,
        Addrs:  []ma.Multiaddr{addr},
    }
    
    // Sign it
    envelope, err := record.Seal(rec, priv)
    if err != nil {
        panic(err)
    }
    
    // Marshal to bytes
    data, err := envelope.Marshal()
    if err != nil {
        panic(err)
    }
    
    // Write to file
    err = os.WriteFile("go-signed-peer-record.bin", data, 0644)
    if err != nil {
        panic(err)
    }
    
    fmt.Println("Generated go-signed-peer-record.bin")
    fmt.Println("PeerID:", pid.String())
    fmt.Println("Domain:", peer.PeerRecordEnvelopeDomain)
    fmt.Println("Payload Type:", peer.PeerRecordEnvelopePayloadType)
}