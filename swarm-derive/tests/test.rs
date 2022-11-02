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
use libp2p::swarm::{dummy, NetworkBehaviour, SwarmEvent};
use libp2p::{identify, ping};
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
        ping: ping::Behaviour,
    }

    #[allow(dead_code, unreachable_code, clippy::diverging_sub_expression)]
    fn foo() {
        let _out_event: <Foo as NetworkBehaviour>::OutEvent = unimplemented!();
        match _out_event {
            FooEvent::Ping(ping::Event { .. }) => {}
        }
    }
}

#[test]
fn two_fields() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo {
        ping: ping::Behaviour,
        identify: identify::Behaviour,
    }

    #[allow(dead_code, unreachable_code, clippy::diverging_sub_expression)]
    fn foo() {
        let _out_event: <Foo as NetworkBehaviour>::OutEvent = unimplemented!();
        match _out_event {
            FooEvent::Ping(ping::Event { .. }) => {}
            FooEvent::Identify(event) => {
                let _: identify::Event = event;
            }
        }
    }
}

#[test]
fn three_fields() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo {
        ping: ping::Behaviour,
        identify: identify::Behaviour,
        kad: libp2p::kad::Kademlia<libp2p::kad::record::store::MemoryStore>,
    }

    #[allow(dead_code, unreachable_code, clippy::diverging_sub_expression)]
    fn foo() {
        let _out_event: <Foo as NetworkBehaviour>::OutEvent = unimplemented!();
        match _out_event {
            FooEvent::Ping(ping::Event { .. }) => {}
            FooEvent::Identify(event) => {
                let _: identify::Event = event;
            }
            FooEvent::Kad(event) => {
                let _: libp2p::kad::KademliaEvent = event;
            }
        }
    }
}

#[test]
fn custom_event() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "MyEvent")]
    struct Foo {
        ping: ping::Behaviour,
        identify: identify::Behaviour,
    }

    #[allow(clippy::large_enum_variant)]
    enum MyEvent {
        Ping(ping::Event),
        Identify(identify::Event),
    }

    impl From<ping::Event> for MyEvent {
        fn from(event: ping::Event) -> Self {
            MyEvent::Ping(event)
        }
    }

    impl From<libp2p::identify::Event> for MyEvent {
        fn from(event: libp2p::identify::Event) -> Self {
            MyEvent::Identify(event)
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
        a: ping::Behaviour,
        b: libp2p::identify::Behaviour,
    }

    #[allow(clippy::large_enum_variant)]
    enum MyEvent {
        Ping(ping::Event),
        Identify(libp2p::identify::Event),
    }

    impl From<ping::Event> for MyEvent {
        fn from(event: ping::Event) -> Self {
            MyEvent::Ping(event)
        }
    }

    impl From<libp2p::identify::Event> for MyEvent {
        fn from(event: libp2p::identify::Event) -> Self {
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
        ping: ping::Behaviour,
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
        ping: ping::Behaviour,
        bar: T,
    }
}

#[test]
fn nested_derives_with_import() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo {
        ping: ping::Behaviour,
    }

    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Bar {
        foo: Foo,
    }

    #[allow(dead_code, unreachable_code, clippy::diverging_sub_expression)]
    fn foo() {
        let _out_event: <Bar as NetworkBehaviour>::OutEvent = unimplemented!();
        match _out_event {
            BarEvent::Foo(FooEvent::Ping(ping::Event { .. })) => {}
        }
    }
}

#[test]
fn custom_event_emit_event_through_poll() {
    #[allow(clippy::large_enum_variant)]
    enum BehaviourOutEvent {
        Ping(ping::Event),
        Identify(identify::Event),
    }

    impl From<ping::Event> for BehaviourOutEvent {
        fn from(event: ping::Event) -> Self {
            BehaviourOutEvent::Ping(event)
        }
    }

    impl From<libp2p::identify::Event> for BehaviourOutEvent {
        fn from(event: libp2p::identify::Event) -> Self {
            BehaviourOutEvent::Identify(event)
        }
    }

    #[allow(dead_code, clippy::large_enum_variant)]
    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "BehaviourOutEvent")]
    struct Foo {
        ping: ping::Behaviour,
        identify: identify::Behaviour,
    }

    #[allow(dead_code, unreachable_code, clippy::diverging_sub_expression)]
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
        identify: identify::Behaviour,
        ping: Toggle<ping::Behaviour>,
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
        ping_or_identify: Either<ping::Behaviour, identify::Behaviour>,
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
        PingOrIdentify(Either<ping::Event, identify::Event>),
    }

    impl From<libp2p::kad::KademliaEvent> for BehaviourOutEvent {
        fn from(event: libp2p::kad::KademliaEvent) -> Self {
            BehaviourOutEvent::Kad(event)
        }
    }

    impl From<Either<ping::Event, identify::Event>> for BehaviourOutEvent {
        fn from(event: Either<ping::Event, identify::Event>) -> Self {
            BehaviourOutEvent::PingOrIdentify(event)
        }
    }

    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "BehaviourOutEvent")]
    struct Foo {
        kad: libp2p::kad::Kademlia<libp2p::kad::record::store::MemoryStore>,
        ping_or_identify: Either<ping::Behaviour, identify::Behaviour>,
    }

    #[allow(dead_code)]
    fn foo() {
        require_net_behaviour::<Foo>();
    }
}

#[test]
fn generated_out_event_derive_debug() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    struct Foo {
        ping: ping::Behaviour,
    }

    fn require_debug<T>()
    where
        T: NetworkBehaviour,
        <T as NetworkBehaviour>::OutEvent: Debug,
    {
    }

    require_debug::<Foo>();
}

#[test]
fn custom_out_event_no_type_parameters() {
    use libp2p::core::connection::ConnectionId;
    use libp2p::swarm::{
        ConnectionHandler, IntoConnectionHandler, NetworkBehaviourAction, PollParameters,
    };
    use libp2p::PeerId;
    use std::task::Context;
    use std::task::Poll;

    pub struct TemplatedBehaviour<T: 'static> {
        _data: T,
    }

    impl<T> NetworkBehaviour for TemplatedBehaviour<T> {
        type ConnectionHandler = dummy::ConnectionHandler;
        type OutEvent = void::Void;

        fn new_handler(&mut self) -> Self::ConnectionHandler {
            dummy::ConnectionHandler
        }

        fn inject_event(
            &mut self,
            _peer: PeerId,
            _connection: ConnectionId,
            message: <<Self::ConnectionHandler as IntoConnectionHandler>::Handler as ConnectionHandler>::OutEvent,
        ) {
            void::unreachable(message);
        }

        fn poll(
            &mut self,
            _ctx: &mut Context,
            _: &mut impl PollParameters,
        ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
            Poll::Pending
        }
    }

    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "OutEvent")]
    struct Behaviour<T: 'static + Send> {
        custom: TemplatedBehaviour<T>,
    }

    #[derive(Debug)]
    enum OutEvent {
        None,
    }

    impl From<void::Void> for OutEvent {
        fn from(_e: void::Void) -> Self {
            Self::None
        }
    }

    require_net_behaviour::<Behaviour<String>>();
    require_net_behaviour::<Behaviour<()>>();
}
