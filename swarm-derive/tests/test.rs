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

use futures::prelude::*;
use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
use libp2p_swarm_derive::*;
use std::fmt::Debug;

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

    #[allow(dead_code)]
    #[allow(unreachable_code)]
    fn foo() {
        let _out_event: <Foo as NetworkBehaviour>::OutEvent = unimplemented!();
        match _out_event {
            FooEvent::Ping(libp2p::ping::Event { .. }) => {}
        }
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

    #[allow(dead_code)]
    #[allow(unreachable_code)]
    fn foo() {
        let _out_event: <Foo as NetworkBehaviour>::OutEvent = unimplemented!();
        match _out_event {
            FooEvent::Ping(libp2p::ping::Event { .. }) => {}
            FooEvent::Identify(event) => {
                let _: libp2p::identify::IdentifyEvent = event;
            }
        }
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

    #[allow(dead_code)]
    #[allow(unreachable_code)]
    fn foo() {
        let _out_event: <Foo as NetworkBehaviour>::OutEvent = unimplemented!();
        match _out_event {
            FooEvent::Ping(libp2p::ping::Event { .. }) => {}
            FooEvent::Identify(event) => {
                let _: libp2p::identify::IdentifyEvent = event;
            }
            FooEvent::Kad(event) => {
                let _: libp2p::kad::KademliaEvent = event;
            }
        }
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

    #[allow(dead_code)]
    #[allow(unreachable_code)]
    fn foo() {
        let _out_event: <Foo as NetworkBehaviour>::OutEvent = unimplemented!();
        match _out_event {
            FooEvent::Ping(libp2p::ping::Event { .. }) => {}
            FooEvent::Kad(event) => {
                let _: libp2p::kad::KademliaEvent = event;
            }
        }
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

    impl Foo {
        fn foo(
            &mut self,
            _: &mut std::task::Context,
            _: &mut impl libp2p::swarm::PollParameters,
        ) -> std::task::Poll<
            libp2p::swarm::NetworkBehaviourAction<
                <Self as NetworkBehaviour>::OutEvent,
                <Self as NetworkBehaviour>::ConnectionHandler,
            >,
        > {
            std::task::Poll::Pending
        }
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
    #[behaviour(out_event = "MyEvent")]
    struct Foo {
        ping: libp2p::ping::Ping,
        identify: libp2p::identify::Identify,
    }

    enum MyEvent {
        Ping(libp2p::ping::PingEvent),
        Identify(libp2p::identify::IdentifyEvent),
    }

    impl From<libp2p::ping::PingEvent> for MyEvent {
        fn from(event: libp2p::ping::PingEvent) -> Self {
            MyEvent::Ping(event)
        }
    }

    impl From<libp2p::identify::IdentifyEvent> for MyEvent {
        fn from(event: libp2p::identify::IdentifyEvent) -> Self {
            MyEvent::Identify(event)
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
    #[behaviour(poll_method = "foo", out_event = "MyEvent")]
    struct Foo {
        ping: libp2p::ping::Ping,
        identify: libp2p::identify::Identify,
    }

    enum MyEvent {
        Ping(libp2p::ping::PingEvent),
        Identify(libp2p::identify::IdentifyEvent),
    }

    impl From<libp2p::ping::PingEvent> for MyEvent {
        fn from(event: libp2p::ping::PingEvent) -> Self {
            MyEvent::Ping(event)
        }
    }

    impl From<libp2p::identify::IdentifyEvent> for MyEvent {
        fn from(event: libp2p::identify::IdentifyEvent) -> Self {
            MyEvent::Identify(event)
        }
    }

    impl Foo {
        fn foo(
            &mut self,
            _: &mut std::task::Context,
            _: &mut impl libp2p::swarm::PollParameters,
        ) -> std::task::Poll<
            libp2p::swarm::NetworkBehaviourAction<
                <Self as NetworkBehaviour>::OutEvent,
                <Self as NetworkBehaviour>::ConnectionHandler,
            >,
        > {
            std::task::Poll::Pending
        }
    }

    #[allow(dead_code)]
    fn foo() {
        require_net_behaviour::<Foo>();
    }
}

#[test]
fn custom_event_mismatching_field_names() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "MyEvent")]
    struct Foo {
        a: libp2p::ping::Ping,
        b: libp2p::identify::Identify,
    }

    enum MyEvent {
        Ping(libp2p::ping::PingEvent),
        Identify(libp2p::identify::IdentifyEvent),
    }

    impl From<libp2p::ping::PingEvent> for MyEvent {
        fn from(event: libp2p::ping::PingEvent) -> Self {
            MyEvent::Ping(event)
        }
    }

    impl From<libp2p::identify::IdentifyEvent> for MyEvent {
        fn from(event: libp2p::identify::IdentifyEvent) -> Self {
            MyEvent::Identify(event)
        }
    }

    #[allow(dead_code)]
    fn foo() {
        require_net_behaviour::<Foo>();
    }
}

#[test]
fn bound() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo<T: Copy + NetworkBehaviour>
    where
        <T as NetworkBehaviour>::OutEvent: Debug,
    {
        ping: libp2p::ping::Ping,
        bar: T,
    }
}

#[test]
fn where_clause() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo<T>
    where
        T: Copy + NetworkBehaviour,
        <T as NetworkBehaviour>::OutEvent: Debug,
    {
        ping: libp2p::ping::Ping,
        bar: T,
    }
}

#[test]
fn nested_derives_with_import() {
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

    #[allow(dead_code)]
    #[allow(unreachable_code)]
    fn foo() {
        let _out_event: <Bar as NetworkBehaviour>::OutEvent = unimplemented!();
        match _out_event {
            BarEvent::Foo(FooEvent::Ping(libp2p::ping::Event { .. })) => {}
        }
    }
}

#[test]
fn custom_event_emit_event_through_poll() {
    enum BehaviourOutEvent {
        Ping(libp2p::ping::PingEvent),
        Identify(libp2p::identify::IdentifyEvent),
    }

    impl From<libp2p::ping::PingEvent> for BehaviourOutEvent {
        fn from(event: libp2p::ping::PingEvent) -> Self {
            BehaviourOutEvent::Ping(event)
        }
    }

    impl From<libp2p::identify::IdentifyEvent> for BehaviourOutEvent {
        fn from(event: libp2p::identify::IdentifyEvent) -> Self {
            BehaviourOutEvent::Identify(event)
        }
    }

    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "BehaviourOutEvent")]
    struct Foo {
        ping: libp2p::ping::Ping,
        identify: libp2p::identify::Identify,
    }

    #[allow(dead_code, unreachable_code)]
    fn bar() {
        require_net_behaviour::<Foo>();

        let mut _swarm: libp2p::Swarm<Foo> = unimplemented!();

        // check that the event is bubbled up all the way to swarm
        let _ = async {
            loop {
                match _swarm.select_next_some().await {
                    SwarmEvent::Behaviour(BehaviourOutEvent::Ping(_)) => break,
                    SwarmEvent::Behaviour(BehaviourOutEvent::Identify(_)) => break,
                    _ => {}
                }
            }
        };
    }
}

#[test]
fn with_toggle() {
    use libp2p::swarm::behaviour::toggle::Toggle;

    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo {
        identify: libp2p::identify::Identify,
        ping: Toggle<libp2p::ping::Ping>,
    }

    #[allow(dead_code)]
    fn foo() {
        require_net_behaviour::<Foo>();
    }
}

#[test]
fn with_either() {
    use either::Either;

    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo {
        kad: libp2p::kad::Kademlia<libp2p::kad::record::store::MemoryStore>,
        ping_or_identify: Either<libp2p::ping::Ping, libp2p::identify::Identify>,
    }

    #[allow(dead_code)]
    fn foo() {
        require_net_behaviour::<Foo>();
    }
}

#[test]
fn custom_event_with_either() {
    use either::Either;

    enum BehaviourOutEvent {
        Kad(libp2p::kad::KademliaEvent),
        PingOrIdentify(Either<libp2p::ping::PingEvent, libp2p::identify::IdentifyEvent>),
    }

    impl From<libp2p::kad::KademliaEvent> for BehaviourOutEvent {
        fn from(event: libp2p::kad::KademliaEvent) -> Self {
            BehaviourOutEvent::Kad(event)
        }
    }

    impl From<Either<libp2p::ping::PingEvent, libp2p::identify::IdentifyEvent>> for BehaviourOutEvent {
        fn from(event: Either<libp2p::ping::PingEvent, libp2p::identify::IdentifyEvent>) -> Self {
            BehaviourOutEvent::PingOrIdentify(event)
        }
    }

    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "BehaviourOutEvent")]
    struct Foo {
        kad: libp2p::kad::Kademlia<libp2p::kad::record::store::MemoryStore>,
        ping_or_identify: Either<libp2p::ping::Ping, libp2p::identify::Identify>,
    }

    #[allow(dead_code)]
    fn foo() {
        require_net_behaviour::<Foo>();
    }
}

#[test]
fn mixed_field_order() {
    struct Foo {}

    #[derive(NetworkBehaviour)]
    pub struct Behaviour {
        #[behaviour(ignore)]
        _foo: Foo,
        _ping: libp2p::ping::Ping,
        #[behaviour(ignore)]
        _foo2: Foo,
        _identify: libp2p::identify::Identify,
        #[behaviour(ignore)]
        _foo3: Foo,
    }

    #[allow(dead_code)]
    fn behaviour() {
        require_net_behaviour::<Behaviour>();
    }
}

#[test]
fn generated_out_event_derive_debug() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo {
        ping: libp2p::ping::Ping,
    }

    fn require_debug<T>()
    where
        T: NetworkBehaviour,
        <T as NetworkBehaviour>::OutEvent: Debug,
    {
    }

    require_debug::<Foo>();
}
