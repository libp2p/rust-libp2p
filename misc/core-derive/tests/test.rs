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
        ping: libp2p::ping::PeriodicPingBehaviour<TSubstream>,
    }
}

#[test]
fn two_fields() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo<TSubstream> {
        ping_dialer: libp2p::ping::PeriodicPingBehaviour<TSubstream>,
        ping_listener: libp2p::ping::PingListenBehaviour<TSubstream>,
    }
}

#[test]
fn three_fields() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo<TSubstream> {
        ping_dialer: libp2p::ping::PeriodicPingBehaviour<TSubstream>,
        ping_listener: libp2p::ping::PingListenBehaviour<TSubstream>,
        identify: libp2p::identify::PeriodicIdentification<TSubstream>,
        #[behaviour(ignore)]
        foo: String,
    }
}

#[test]
fn event_handler() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    // TODO: remove the generics requirements once identify no longer requires them
    struct Foo<TSubstream: libp2p::tokio_io::AsyncRead + libp2p::tokio_io::AsyncWrite + Send + Sync + 'static> {
        #[behaviour(handler = "foo")]
        identify: libp2p::identify::PeriodicIdentifyBehaviour<TSubstream>,
    }

    impl<TSubstream: libp2p::tokio_io::AsyncRead + libp2p::tokio_io::AsyncWrite + Send + Sync + 'static> Foo<TSubstream> {
        // TODO: for some reason, the parameter cannot be `PeriodicIdentifyBehaviourEvent` or we
        //       get a compilation error ; figure out why or open an issue to Rust
        fn foo(&mut self, ev: <libp2p::identify::PeriodicIdentifyBehaviour<TSubstream> as libp2p::core::nodes::NetworkBehaviour>::OutEvent) {
            let libp2p::identify::PeriodicIdentifyBehaviourEvent::Identified { .. } = ev;
        }
    }
}

#[test]
fn custom_polling() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(poll_method = "foo")]
    struct Foo<TSubstream> {
        ping: libp2p::ping::PeriodicPingBehaviour<TSubstream>,
        identify: libp2p::identify::PeriodicIdentifyBehaviour<TSubstream>,
    }

    impl<TSubstream> Foo<TSubstream> {
        fn foo<T>(&mut self) -> libp2p::futures::Async<libp2p::core::nodes::NetworkBehaviourAction<T, ()>> { libp2p::futures::Async::NotReady }
    }
}

#[test]
fn custom_event_no_polling() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "String")]
    struct Foo<TSubstream> {
        ping: libp2p::ping::PeriodicPingBehaviour<TSubstream>,
        identify: libp2p::identify::PeriodicIdentifyBehaviour<TSubstream>,
    }
}

#[test]
fn custom_event_and_polling() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(poll_method = "foo", out_event = "String")]
    struct Foo<TSubstream> {
        ping: libp2p::ping::PeriodicPingBehaviour<TSubstream>,
        identify: libp2p::identify::PeriodicIdentifyBehaviour<TSubstream>,
    }

    impl<TSubstream> Foo<TSubstream> {
        fn foo<T>(&mut self) -> libp2p::futures::Async<libp2p::core::nodes::NetworkBehaviourAction<T, String>> { libp2p::futures::Async::NotReady }
    }
}
