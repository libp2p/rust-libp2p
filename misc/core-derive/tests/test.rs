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

#[macro_use]
extern crate libp2p;

// TODO: doesn't compile
/*#[test]
fn empty() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo {}
}*/

#[test]
fn one_field() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo<TSubstream> {
        ping: libp2p::ping::PeriodicPingHandler<TSubstream>,
    }
}

// TODO: not enough implementations of NetworkBehaviour exist to write tests
/*#[test]
fn two_fields() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo<TSubstream> {
        ping_dialer: libp2p::ping::PeriodicPingHandler<TSubstream>,
        ping_listener: libp2p::ping::PingListenHandler<TSubstream>,
    }
}

#[test]
fn three_fields() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo<TSubstream> {
        ping_dialer: libp2p::ping::PeriodicPingHandler<TSubstream>,
        ping_listener: libp2p::ping::PingListenHandler<TSubstream>,
        identify: libp2p::identify::PeriodicIdentification<TSubstream>,
    }
}*/
