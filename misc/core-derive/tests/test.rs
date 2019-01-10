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
extern crate void;

/// Small utility to check that a type implements `NetworkBehaviour`.
#[allow(dead_code)]
fn require_net_behaviour<T: libp2p::core::swarm::NetworkBehaviour<libp2p::core::topology::MemoryTopology>>() {}

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
        ping: libp2p::ping::Ping<TSubstream>,
    }

    impl<TSubstream> libp2p::core::swarm::NetworkBehaviourEventProcess<libp2p::ping::PingEvent> for Foo<TSubstream> {
        fn inject_event(&mut self, _: libp2p::ping::PingEvent) {
        }
    }

    fn foo<TSubstream: libp2p::tokio_io::AsyncRead + libp2p::tokio_io::AsyncWrite>() {
        require_net_behaviour::<Foo<TSubstream>>();
    }
}

#[test]
fn two_fields() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo<TSubstream> {
        ping: libp2p::ping::Ping<TSubstream>,
        identify: libp2p::identify::Identify<TSubstream>,
    }

    impl<TSubstream> libp2p::core::swarm::NetworkBehaviourEventProcess<libp2p::identify::IdentifyEvent> for Foo<TSubstream> {
        fn inject_event(&mut self, _: libp2p::identify::IdentifyEvent) {
        }
    }

    impl<TSubstream> libp2p::core::swarm::NetworkBehaviourEventProcess<libp2p::ping::PingEvent> for Foo<TSubstream> {
        fn inject_event(&mut self, _: libp2p::ping::PingEvent) {
        }
    }

    fn foo<TSubstream: libp2p::tokio_io::AsyncRead + libp2p::tokio_io::AsyncWrite>() {
        require_net_behaviour::<Foo<TSubstream>>();
    }
}

#[test]
fn three_fields() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo<TSubstream> {
        ping: libp2p::ping::Ping<TSubstream>,
        identify: libp2p::identify::Identify<TSubstream>,
        kad: libp2p::kad::Kademlia<TSubstream>,
        #[behaviour(ignore)]
        foo: String,
    }

    impl<TSubstream> libp2p::core::swarm::NetworkBehaviourEventProcess<libp2p::ping::PingEvent> for Foo<TSubstream> {
        fn inject_event(&mut self, _: libp2p::ping::PingEvent) {
        }
    }

    impl<TSubstream> libp2p::core::swarm::NetworkBehaviourEventProcess<libp2p::identify::IdentifyEvent> for Foo<TSubstream> {
        fn inject_event(&mut self, _: libp2p::identify::IdentifyEvent) {
        }
    }

    impl<TSubstream> libp2p::core::swarm::NetworkBehaviourEventProcess<libp2p::kad::KademliaOut> for Foo<TSubstream> {
        fn inject_event(&mut self, _: libp2p::kad::KademliaOut) {
        }
    }

    fn foo<TSubstream: libp2p::tokio_io::AsyncRead + libp2p::tokio_io::AsyncWrite>() {
        require_net_behaviour::<Foo<TSubstream>>();
    }
}

#[test]
fn custom_polling() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(poll_method = "foo")]
    struct Foo<TSubstream> {
        ping: libp2p::ping::Ping<TSubstream>,
        identify: libp2p::identify::Identify<TSubstream>,
    }

    impl<TSubstream> libp2p::core::swarm::NetworkBehaviourEventProcess<libp2p::ping::PingEvent> for Foo<TSubstream> {
        fn inject_event(&mut self, _: libp2p::ping::PingEvent) {
        }
    }

    impl<TSubstream> libp2p::core::swarm::NetworkBehaviourEventProcess<libp2p::identify::IdentifyEvent> for Foo<TSubstream> {
        fn inject_event(&mut self, _: libp2p::identify::IdentifyEvent) {
        }
    }

    impl<TSubstream> Foo<TSubstream> {
        fn foo<T>(&mut self) -> libp2p::futures::Async<libp2p::core::swarm::NetworkBehaviourAction<T, ()>> { libp2p::futures::Async::NotReady }
    }

    fn foo<TSubstream: libp2p::tokio_io::AsyncRead + libp2p::tokio_io::AsyncWrite>() {
        require_net_behaviour::<Foo<TSubstream>>();
    }
}

#[test]
fn custom_event_no_polling() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "String")]
    struct Foo<TSubstream> {
        ping: libp2p::ping::Ping<TSubstream>,
        identify: libp2p::identify::Identify<TSubstream>,
    }

    impl<TSubstream> libp2p::core::swarm::NetworkBehaviourEventProcess<libp2p::ping::PingEvent> for Foo<TSubstream> {
        fn inject_event(&mut self, _: libp2p::ping::PingEvent) {
        }
    }

    impl<TSubstream> libp2p::core::swarm::NetworkBehaviourEventProcess<libp2p::identify::IdentifyEvent> for Foo<TSubstream> {
        fn inject_event(&mut self, _: libp2p::identify::IdentifyEvent) {
        }
    }

    fn foo<TSubstream: libp2p::tokio_io::AsyncRead + libp2p::tokio_io::AsyncWrite>() {
        require_net_behaviour::<Foo<TSubstream>>();
    }
}

#[test]
fn custom_event_and_polling() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(poll_method = "foo", out_event = "String")]
    struct Foo<TSubstream> {
        ping: libp2p::ping::Ping<TSubstream>,
        identify: libp2p::identify::Identify<TSubstream>,
    }

    impl<TSubstream> libp2p::core::swarm::NetworkBehaviourEventProcess<libp2p::ping::PingEvent> for Foo<TSubstream> {
        fn inject_event(&mut self, _: libp2p::ping::PingEvent) {
        }
    }

    impl<TSubstream> libp2p::core::swarm::NetworkBehaviourEventProcess<libp2p::identify::IdentifyEvent> for Foo<TSubstream> {
        fn inject_event(&mut self, _: libp2p::identify::IdentifyEvent) {
        }
    }

    impl<TSubstream> Foo<TSubstream> {
        fn foo<T>(&mut self) -> libp2p::futures::Async<libp2p::core::swarm::NetworkBehaviourAction<T, String>> { libp2p::futures::Async::NotReady }
    }

    fn foo<TSubstream: libp2p::tokio_io::AsyncRead + libp2p::tokio_io::AsyncWrite>() {
        require_net_behaviour::<Foo<TSubstream>>();
    }
}
