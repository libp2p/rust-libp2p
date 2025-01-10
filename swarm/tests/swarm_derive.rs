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

use std::fmt::Debug;

use futures::StreamExt;
use libp2p_core::{transport::PortUse, Endpoint, Multiaddr};
use libp2p_identify as identify;
use libp2p_ping as ping;
use libp2p_swarm::{
    behaviour::FromSwarm, dummy, ConnectionDenied, NetworkBehaviour, SwarmEvent, THandler,
    THandlerInEvent, THandlerOutEvent,
};

/// Small utility to check that a type implements `NetworkBehaviour`.
#[allow(dead_code)]
fn require_net_behaviour<T: libp2p_swarm::NetworkBehaviour>() {}

// TODO: doesn't compile
// #[test]
// fn empty() {
// #[allow(dead_code)]
// #[derive(NetworkBehaviour)]
// struct Foo {}
// }

#[test]
fn one_field() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
    struct Foo {
        ping: ping::Behaviour,
    }

    #[allow(
        dead_code,
        unreachable_code,
        clippy::diverging_sub_expression,
        clippy::used_underscore_binding
    )]
    fn foo() {
        let _out_event: <Foo as NetworkBehaviour>::ToSwarm = unimplemented!();
        match _out_event {
            FooEvent::Ping(ping::Event { .. }) => {}
        }
    }
}

#[test]
fn two_fields() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
    struct Foo {
        ping: ping::Behaviour,
        identify: identify::Behaviour,
    }

    #[allow(
        dead_code,
        unreachable_code,
        clippy::diverging_sub_expression,
        clippy::used_underscore_binding
    )]
    fn foo() {
        let _out_event: <Foo as NetworkBehaviour>::ToSwarm = unimplemented!();
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
    #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
    struct Foo {
        ping: ping::Behaviour,
        identify: identify::Behaviour,
        kad: libp2p_kad::Behaviour<libp2p_kad::store::MemoryStore>,
    }

    #[allow(
        dead_code,
        unreachable_code,
        clippy::diverging_sub_expression,
        clippy::used_underscore_binding
    )]
    fn foo() {
        let _out_event: <Foo as NetworkBehaviour>::ToSwarm = unimplemented!();
        match _out_event {
            FooEvent::Ping(ping::Event { .. }) => {}
            FooEvent::Identify(event) => {
                let _: identify::Event = event;
            }
            FooEvent::Kad(event) => {
                let _: libp2p_kad::Event = event;
            }
        }
    }
}

#[test]
fn custom_event() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(to_swarm = "MyEvent", prelude = "libp2p_swarm::derive_prelude")]
    struct Foo {
        ping: ping::Behaviour,
        identify: identify::Behaviour,
    }

    #[allow(clippy::large_enum_variant)]
    enum MyEvent {
        Ping,
        Identify,
    }

    impl From<ping::Event> for MyEvent {
        fn from(_event: ping::Event) -> Self {
            MyEvent::Ping
        }
    }

    impl From<identify::Event> for MyEvent {
        fn from(_event: identify::Event) -> Self {
            MyEvent::Identify
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
    #[behaviour(to_swarm = "MyEvent", prelude = "libp2p_swarm::derive_prelude")]
    struct Foo {
        a: ping::Behaviour,
        b: identify::Behaviour,
    }

    #[allow(clippy::large_enum_variant)]
    enum MyEvent {
        Ping,
        Identify,
    }

    impl From<ping::Event> for MyEvent {
        fn from(_event: ping::Event) -> Self {
            MyEvent::Ping
        }
    }

    impl From<identify::Event> for MyEvent {
        fn from(_event: identify::Event) -> Self {
            MyEvent::Identify
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
    #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
    struct Foo<T: Copy + NetworkBehaviour>
    where
        <T as NetworkBehaviour>::ToSwarm: Debug,
    {
        ping: ping::Behaviour,
        bar: T,
    }
}

#[test]
fn where_clause() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
    struct Foo<T>
    where
        T: Copy + NetworkBehaviour,
        <T as NetworkBehaviour>::ToSwarm: Debug,
    {
        ping: ping::Behaviour,
        bar: T,
    }
}

#[test]
fn nested_derives_with_import() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
    struct Foo {
        ping: ping::Behaviour,
    }

    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
    struct Bar {
        foo: Foo,
    }

    #[allow(
        dead_code,
        unreachable_code,
        clippy::diverging_sub_expression,
        clippy::used_underscore_binding
    )]
    fn foo() {
        let _out_event: <Bar as NetworkBehaviour>::ToSwarm = unimplemented!();
        match _out_event {
            BarEvent::Foo(FooEvent::Ping(ping::Event { .. })) => {}
        }
    }
}

#[test]
fn custom_event_emit_event_through_poll() {
    #[allow(clippy::large_enum_variant)]
    enum BehaviourOutEvent {
        Ping,
        Identify,
    }

    impl From<ping::Event> for BehaviourOutEvent {
        fn from(_event: ping::Event) -> Self {
            BehaviourOutEvent::Ping
        }
    }

    impl From<identify::Event> for BehaviourOutEvent {
        fn from(_event: identify::Event) -> Self {
            BehaviourOutEvent::Identify
        }
    }

    #[allow(dead_code, clippy::large_enum_variant)]
    #[derive(NetworkBehaviour)]
    #[behaviour(
        to_swarm = "BehaviourOutEvent",
        prelude = "libp2p_swarm::derive_prelude"
    )]
    struct Foo {
        ping: ping::Behaviour,
        identify: identify::Behaviour,
    }

    #[allow(
        dead_code,
        unreachable_code,
        clippy::diverging_sub_expression,
        clippy::used_underscore_binding
    )]
    async fn bar() {
        require_net_behaviour::<Foo>();

        let mut _swarm: libp2p_swarm::Swarm<Foo> = unimplemented!();

        // check that the event is bubbled up all the way to swarm
        loop {
            match _swarm.select_next_some().await {
                SwarmEvent::Behaviour(BehaviourOutEvent::Ping) => break,
                SwarmEvent::Behaviour(BehaviourOutEvent::Identify) => break,
                _ => {}
            }
        }
    }
}

#[test]
fn with_toggle() {
    use libp2p_swarm::behaviour::toggle::Toggle;

    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
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
    #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
    struct Foo {
        kad: libp2p_kad::Behaviour<libp2p_kad::store::MemoryStore>,
        ping_or_identify: Either<ping::Behaviour, identify::Behaviour>,
    }

    #[allow(dead_code)]
    fn foo() {
        require_net_behaviour::<Foo>();
    }
}

#[test]
fn with_generics() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
    struct Foo<A, B> {
        a: A,
        b: B,
    }

    #[allow(dead_code)]
    fn foo() {
        require_net_behaviour::<
            Foo<libp2p_kad::Behaviour<libp2p_kad::store::MemoryStore>, libp2p_ping::Behaviour>,
        >();
    }
}

#[test]
fn with_generics_mixed() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
    struct Foo<A> {
        a: A,
        ping: libp2p_ping::Behaviour,
    }

    #[allow(dead_code)]
    fn foo() {
        require_net_behaviour::<Foo<libp2p_kad::Behaviour<libp2p_kad::store::MemoryStore>>>();
    }
}

#[test]
fn with_generics_constrained() {
    use std::task::{Context, Poll};
    trait Mark {}
    struct Marked;
    impl Mark for Marked {}

    /// A struct with a generic constraint, for which we manually implement `NetworkBehaviour`.
    #[allow(dead_code)]
    struct Bar<A: Mark> {
        a: A,
    }

    impl<A: Mark + 'static> NetworkBehaviour for Bar<A> {
        type ConnectionHandler = dummy::ConnectionHandler;
        type ToSwarm = std::convert::Infallible;

        fn handle_established_inbound_connection(
            &mut self,
            _: libp2p_swarm::ConnectionId,
            _: libp2p_identity::PeerId,
            _: &Multiaddr,
            _: &Multiaddr,
        ) -> Result<THandler<Self>, ConnectionDenied> {
            Ok(dummy::ConnectionHandler)
        }

        fn handle_established_outbound_connection(
            &mut self,
            _: libp2p_swarm::ConnectionId,
            _: libp2p_identity::PeerId,
            _: &Multiaddr,
            _: Endpoint,
            _: PortUse,
        ) -> Result<THandler<Self>, ConnectionDenied> {
            Ok(dummy::ConnectionHandler)
        }

        fn on_swarm_event(&mut self, _event: FromSwarm) {}

        fn on_connection_handler_event(
            &mut self,
            _: libp2p_identity::PeerId,
            _: libp2p_swarm::ConnectionId,
            _: THandlerOutEvent<Self>,
        ) {
        }

        fn poll(
            &mut self,
            _: &mut Context<'_>,
        ) -> Poll<libp2p_swarm::ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
            Poll::Pending
        }
    }

    /// A struct which uses the above, inheriting the generic constraint,
    /// for which we want to derive the `NetworkBehaviour`.
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
    struct Foo1<A: Mark> {
        bar: Bar<A>,
    }

    /// A struct which uses the above, inheriting the generic constraint,
    /// for which we want to derive the `NetworkBehaviour`.
    ///
    /// Using a where clause instead of inline constraint.
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
    struct Foo2<A>
    where
        A: Mark,
    {
        bar: Bar<A>,
    }

    #[allow(dead_code)]
    fn foo() {
        require_net_behaviour::<Foo1<Marked>>();
        require_net_behaviour::<Foo2<Marked>>();
    }
}

#[test]
fn custom_event_with_either() {
    use either::Either;

    enum BehaviourOutEvent {
        Kad,
        PingOrIdentify,
    }

    impl From<libp2p_kad::Event> for BehaviourOutEvent {
        fn from(_event: libp2p_kad::Event) -> Self {
            BehaviourOutEvent::Kad
        }
    }

    impl From<Either<ping::Event, identify::Event>> for BehaviourOutEvent {
        fn from(_event: Either<ping::Event, identify::Event>) -> Self {
            BehaviourOutEvent::PingOrIdentify
        }
    }

    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(
        to_swarm = "BehaviourOutEvent",
        prelude = "libp2p_swarm::derive_prelude"
    )]
    struct Foo {
        kad: libp2p_kad::Behaviour<libp2p_kad::store::MemoryStore>,
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
    #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
    struct Foo {
        ping: ping::Behaviour,
    }

    fn require_debug<T>()
    where
        T: NetworkBehaviour,
        <T as NetworkBehaviour>::ToSwarm: Debug,
    {
    }

    require_debug::<Foo>();
}

#[test]
fn multiple_behaviour_attributes() {
    #[allow(dead_code)]
    #[derive(NetworkBehaviour)]
    #[behaviour(to_swarm = "FooEvent")]
    #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
    struct Foo {
        ping: ping::Behaviour,
    }

    require_net_behaviour::<Foo>();

    struct FooEvent;

    impl From<ping::Event> for FooEvent {
        fn from(_: ping::Event) -> Self {
            unimplemented!()
        }
    }
}

#[test]
fn custom_out_event_no_type_parameters() {
    use std::task::{Context, Poll};

    use libp2p_identity::PeerId;
    use libp2p_swarm::{ConnectionId, ToSwarm};

    pub(crate) struct TemplatedBehaviour<T: 'static> {
        _data: T,
    }

    impl<T> NetworkBehaviour for TemplatedBehaviour<T> {
        type ConnectionHandler = dummy::ConnectionHandler;
        type ToSwarm = std::convert::Infallible;

        fn handle_established_inbound_connection(
            &mut self,
            _: ConnectionId,
            _: PeerId,
            _: &Multiaddr,
            _: &Multiaddr,
        ) -> Result<THandler<Self>, ConnectionDenied> {
            Ok(dummy::ConnectionHandler)
        }

        fn handle_established_outbound_connection(
            &mut self,
            _: ConnectionId,
            _: PeerId,
            _: &Multiaddr,
            _: Endpoint,
            _: PortUse,
        ) -> Result<THandler<Self>, ConnectionDenied> {
            Ok(dummy::ConnectionHandler)
        }

        fn on_connection_handler_event(
            &mut self,
            _peer: PeerId,
            _connection: ConnectionId,
            message: THandlerOutEvent<Self>,
        ) {
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            libp2p_core::util::unreachable(message);
        }

        fn poll(
            &mut self,
            _: &mut Context<'_>,
        ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
            Poll::Pending
        }

        fn on_swarm_event(&mut self, _event: FromSwarm) {}
    }

    #[derive(NetworkBehaviour)]
    #[behaviour(to_swarm = "OutEvent", prelude = "libp2p_swarm::derive_prelude")]
    struct Behaviour<T: 'static + Send> {
        custom: TemplatedBehaviour<T>,
    }

    #[derive(Debug)]
    enum OutEvent {
        None,
    }

    impl From<std::convert::Infallible> for OutEvent {
        fn from(_e: std::convert::Infallible) -> Self {
            Self::None
        }
    }

    require_net_behaviour::<Behaviour<String>>();
    require_net_behaviour::<Behaviour<()>>();
}

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/fail/*.rs");
}
