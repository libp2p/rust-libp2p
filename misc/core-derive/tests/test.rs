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

use libp2p_core_derive::*;

/// Small utility to check that a type implements `NetworkBehaviour`.
#[allow(dead_code)]
fn require_net_behaviour<T: libp2p::swarm::NetworkBehaviour>() {}

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
    struct Foo {
        ping: libp2p::ping::Ping,
    }

    impl libp2p::swarm::NetworkBehaviourEventProcess<libp2p::ping::PingEvent> for Foo {
        fn inject_event(&mut self, _: libp2p::ping::PingEvent) {
        }
    }

    #[allow(dead_code)]
    fn foo() {
        require_net_behaviour::<Foo>();
    }
}

#[test]
fn two_fields() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo {
        ping: libp2p::ping::Ping,
        identify: libp2p::identify::Identify,
    }

    impl libp2p::swarm::NetworkBehaviourEventProcess<libp2p::identify::IdentifyEvent> for Foo {
        fn inject_event(&mut self, _: libp2p::identify::IdentifyEvent) {
        }
    }

    impl libp2p::swarm::NetworkBehaviourEventProcess<libp2p::ping::PingEvent> for Foo {
        fn inject_event(&mut self, _: libp2p::ping::PingEvent) {
        }
    }

    #[allow(dead_code)]
    fn foo() {
        require_net_behaviour::<Foo>();
    }
}

#[test]
fn three_fields() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo {
        ping: libp2p::ping::Ping,
        identify: libp2p::identify::Identify,
        kad: libp2p::kad::Kademlia<libp2p::kad::record::store::MemoryStore>,
        #[behaviour(ignore)]
        foo: String,
    }

    impl libp2p::swarm::NetworkBehaviourEventProcess<libp2p::ping::PingEvent> for Foo {
        fn inject_event(&mut self, _: libp2p::ping::PingEvent) {
        }
    }

    impl libp2p::swarm::NetworkBehaviourEventProcess<libp2p::identify::IdentifyEvent> for Foo {
        fn inject_event(&mut self, _: libp2p::identify::IdentifyEvent) {
        }
    }

    impl libp2p::swarm::NetworkBehaviourEventProcess<libp2p::kad::KademliaEvent> for Foo {
        fn inject_event(&mut self, _: libp2p::kad::KademliaEvent) {
        }
    }

    #[allow(dead_code)]
    fn foo() {
        require_net_behaviour::<Foo>();
    }
}

#[test]
fn three_fields_non_last_ignored() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo {
        ping: libp2p::ping::Ping,
        #[behaviour(ignore)]
        identify: String,
        kad: libp2p::kad::Kademlia<libp2p::kad::record::store::MemoryStore>,
    }

    impl libp2p::swarm::NetworkBehaviourEventProcess<libp2p::ping::PingEvent> for Foo {
        fn inject_event(&mut self, _: libp2p::ping::PingEvent) {
        }
    }

    impl libp2p::swarm::NetworkBehaviourEventProcess<libp2p::kad::KademliaEvent> for Foo {
        fn inject_event(&mut self, _: libp2p::kad::KademliaEvent) {
        }
    }

    #[allow(dead_code)]
    fn foo() {
        require_net_behaviour::<Foo>();
    }
}

#[test]
fn custom_polling() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(poll_method = "foo")]
    struct Foo {
        ping: libp2p::ping::Ping,
        identify: libp2p::identify::Identify,
    }

    impl libp2p::swarm::NetworkBehaviourEventProcess<libp2p::ping::PingEvent> for Foo {
        fn inject_event(&mut self, _: libp2p::ping::PingEvent) {
        }
    }

    impl libp2p::swarm::NetworkBehaviourEventProcess<libp2p::identify::IdentifyEvent> for Foo {
        fn inject_event(&mut self, _: libp2p::identify::IdentifyEvent) {
        }
    }

    impl Foo {
        fn foo<T>(&mut self, _: &mut std::task::Context, _: &mut impl libp2p::swarm::PollParameters) -> std::task::Poll<libp2p::swarm::NetworkBehaviourAction<T, ()>> { std::task::Poll::Pending }
    }

    #[allow(dead_code)]
    fn foo() {
        require_net_behaviour::<Foo>();
    }
}

#[test]
fn custom_event_no_polling() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "Vec<String>")]
    struct Foo {
        ping: libp2p::ping::Ping,
        identify: libp2p::identify::Identify,
    }

    impl libp2p::swarm::NetworkBehaviourEventProcess<libp2p::ping::PingEvent> for Foo {
        fn inject_event(&mut self, _: libp2p::ping::PingEvent) {
        }
    }

    impl libp2p::swarm::NetworkBehaviourEventProcess<libp2p::identify::IdentifyEvent> for Foo {
        fn inject_event(&mut self, _: libp2p::identify::IdentifyEvent) {
        }
    }

    #[allow(dead_code)]
    fn foo() {
        require_net_behaviour::<Foo>();
    }
}

#[test]
fn custom_event_and_polling() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(poll_method = "foo", out_event = "String")]
    struct Foo {
        ping: libp2p::ping::Ping,
        identify: libp2p::identify::Identify,
    }

    impl libp2p::swarm::NetworkBehaviourEventProcess<libp2p::ping::PingEvent> for Foo {
        fn inject_event(&mut self, _: libp2p::ping::PingEvent) {
        }
    }

    impl libp2p::swarm::NetworkBehaviourEventProcess<libp2p::identify::IdentifyEvent> for Foo {
        fn inject_event(&mut self, _: libp2p::identify::IdentifyEvent) {
        }
    }

    impl Foo {
        fn foo<T>(&mut self, _: &mut std::task::Context, _: &mut impl libp2p::swarm::PollParameters) -> std::task::Poll<libp2p::swarm::NetworkBehaviourAction<T, String>> { std::task::Poll::Pending }
    }

    #[allow(dead_code)]
    fn foo() {
        require_net_behaviour::<Foo>();
    }
}

#[test]
fn where_clause() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo<T: Copy> {
        ping: libp2p::ping::Ping,
        bar: T,
    }
}

#[test]
fn nested_derives_with_import() {
    use libp2p::swarm::NetworkBehaviourEventProcess;

    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo {
        ping: libp2p::ping::Ping,
    }

    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Bar {
        foo: Foo,
    }

    impl NetworkBehaviourEventProcess<libp2p::ping::PingEvent> for Foo {
        fn inject_event(&mut self, _: libp2p::ping::PingEvent) {
        }
    }

    impl NetworkBehaviourEventProcess<()> for Bar {
        fn inject_event(&mut self, _: ()) {
        }
    }

    #[allow(dead_code)]
    fn bar() {
        require_net_behaviour::<Bar>();
    }
}
