// Copyright 2021 COMIT Network.
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

use futures::stream::FusedStream;
use futures::StreamExt;
use futures::{future, Stream};
use libp2p::swarm::SwarmEvent;
use std::fmt::Debug;
use std::time::Duration;

pub async fn await_events_or_timeout<Event1, Event2, Error1, Error2>(
    swarm_1: &mut (impl Stream<Item = SwarmEvent<Event1, Error1>> + FusedStream + Unpin),
    swarm_2: &mut (impl Stream<Item = SwarmEvent<Event2, Error2>> + FusedStream + Unpin),
) -> (SwarmEvent<Event1, Error1>, SwarmEvent<Event2, Error2>)
where
    SwarmEvent<Event1, Error1>: Debug,
    SwarmEvent<Event2, Error2>: Debug,
{
    tokio::time::timeout(
        Duration::from_secs(30),
        future::join(
            swarm_1
                .inspect(|event| log::debug!("Swarm1 emitted {:?}", event))
                .select_next_some(),
            swarm_2
                .inspect(|event| log::debug!("Swarm2 emitted {:?}", event))
                .select_next_some(),
        ),
    )
    .await
    .expect("network behaviours to emit an event within 30 seconds")
}

#[macro_export]
macro_rules! assert_behaviour_events {
    ($swarm: ident: $pat: pat, || $body: block) => {
        match $swarm.next_or_timeout().await {
            libp2p_swarm::SwarmEvent::Behaviour($pat) => $body,
            _ => panic!("Unexpected combination of events emitted, check logs for details"),
        }
    };
    ($swarm1: ident: $pat1: pat, $swarm2: ident: $pat2: pat, || $body: block) => {
        match await_events_or_timeout(&mut $swarm1, &mut $swarm2).await {
            (
                libp2p_swarm::SwarmEvent::Behaviour($pat1),
                libp2p_swarm::SwarmEvent::Behaviour($pat2),
            ) => $body,
            _ => panic!("Unexpected combination of events emitted, check logs for details"),
        }
    };
}
