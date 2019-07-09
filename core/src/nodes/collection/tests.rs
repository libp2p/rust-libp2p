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

#![cfg(test)]

use super::*;
use assert_matches::assert_matches;
use futures::future;
use crate::tests::dummy_muxer::{DummyMuxer, DummyConnectionState};
use crate::tests::dummy_handler::{Handler, InEvent, OutEvent, HandlerState};
use tokio::runtime::current_thread::Runtime;
use tokio::runtime::Builder;
use crate::nodes::NodeHandlerEvent;
use std::{io, sync::Arc};
use parking_lot::Mutex;

type TestCollectionStream = CollectionStream<InEvent, OutEvent, Handler, io::Error, io::Error, ()>;

#[test]
fn has_connection_is_false_before_a_connection_has_been_made() {
    let cs = TestCollectionStream::new();
    let peer_id = PeerId::random();
    assert!(!cs.has_connection(&peer_id));
}

#[test]
fn connections_is_empty_before_connecting() {
    let cs = TestCollectionStream::new();
    assert!(cs.connections().next().is_none());
}

#[test]
fn retrieving_a_peer_is_none_if_peer_is_missing_or_not_connected() {
    let mut cs = TestCollectionStream::new();
    let peer_id = PeerId::random();
    assert!(cs.peer_mut(&peer_id).is_none());

    let handler = Handler::default();
    let fut = future::ok((peer_id.clone(), DummyMuxer::new()));
    cs.add_reach_attempt(fut, handler);
    assert!(cs.peer_mut(&peer_id).is_none()); // task is pending
}

#[test]
fn collection_stream_reaches_the_nodes() {
    let mut cs = TestCollectionStream::new();
    let peer_id = PeerId::random();

    let mut muxer = DummyMuxer::new();
    muxer.set_inbound_connection_state(DummyConnectionState::Pending);
    muxer.set_outbound_connection_state(DummyConnectionState::Opened);

    let fut = future::ok((peer_id, muxer));
    cs.add_reach_attempt(fut, Handler::default());
    let mut rt = Runtime::new().unwrap();
    let mut poll_count = 0;
    let fut = future::poll_fn(move || -> Poll<(), ()> {
        poll_count += 1;
        let event = cs.poll();
        match poll_count {
            1 => assert_matches!(event, Async::NotReady),
            2 => {
                assert_matches!(event, Async::Ready(CollectionEvent::NodeReached(_)));
                return Ok(Async::Ready(())); // stop
            }
            _ => unreachable!()
        }
        Ok(Async::NotReady)
    });
    rt.block_on(fut).unwrap();
}

#[test]
fn accepting_a_node_yields_new_entry() {
    let mut cs = TestCollectionStream::new();
    let peer_id = PeerId::random();
    let fut = future::ok((peer_id.clone(), DummyMuxer::new()));
    cs.add_reach_attempt(fut, Handler::default());

    let mut rt = Runtime::new().unwrap();
    let mut poll_count = 0;
    let fut = future::poll_fn(move || -> Poll<(), ()> {
        poll_count += 1;
        {
            let event = cs.poll();
            match poll_count {
                1 => {
                    assert_matches!(event, Async::NotReady);
                    return Ok(Async::NotReady)
                }
                2 => {
                    assert_matches!(event, Async::Ready(CollectionEvent::NodeReached(reach_ev)) => {
                        let (accept_ev, accepted_peer_id) = reach_ev.accept(());
                        assert_eq!(accepted_peer_id, peer_id);
                        assert_matches!(accept_ev, CollectionNodeAccept::NewEntry);
                    });
                }
                _ => unreachable!()
            }
        }
        assert!(cs.peer_mut(&peer_id).is_some(), "peer is not in the list");
        assert!(cs.has_connection(&peer_id), "peer is not connected");
        assert_eq!(cs.connections().collect::<Vec<&PeerId>>(), vec![&peer_id]);
        Ok(Async::Ready(()))
    });
    rt.block_on(fut).expect("running the future works");
}

#[test]
fn events_in_a_node_reaches_the_collection_stream() {
    let cs = Arc::new(Mutex::new(TestCollectionStream::new()));
    let task_peer_id = PeerId::random();

    let mut handler = Handler::default();
    handler.state = Some(HandlerState::Ready(NodeHandlerEvent::Custom(OutEvent::Custom("init"))));
    let handler_states = vec![
        HandlerState::Err,
        HandlerState::Ready(NodeHandlerEvent::Custom(OutEvent::Custom("from handler 3") )),
        HandlerState::Ready(NodeHandlerEvent::Custom(OutEvent::Custom("from handler 2") )),
        HandlerState::Ready(NodeHandlerEvent::Custom(OutEvent::Custom("from handler 1") )),
    ];
    handler.next_states = handler_states;

    let mut muxer = DummyMuxer::new();
    muxer.set_inbound_connection_state(DummyConnectionState::Pending);
    muxer.set_outbound_connection_state(DummyConnectionState::Opened);

    let fut = future::ok((task_peer_id.clone(), muxer));
    cs.lock().add_reach_attempt(fut, handler);

    let mut rt = Builder::new().core_threads(1).build().unwrap();

    let cs_fut = cs.clone();
    rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
        let mut cs = cs_fut.lock();
        assert_matches!(cs.poll(), Async::NotReady);
        Ok(Async::Ready(()))
    })).expect("tokio works");

    let cs2 = cs.clone();
    rt.block_on(future::poll_fn(move || {
        if cs2.lock().start_broadcast(&InEvent::NextState).is_not_ready() {
            Ok::<_, ()>(Async::NotReady)
        } else {
            Ok(Async::Ready(()))
        }
    })).unwrap();
    let cs_fut = cs.clone();
    rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
        let mut cs = cs_fut.lock();
        if cs.complete_broadcast().is_not_ready() {
            return Ok(Async::NotReady)
        }
        assert_matches!(cs.poll(), Async::Ready(CollectionEvent::NodeReached(reach_ev)) => {
            reach_ev.accept(());
        });
        Ok(Async::Ready(()))
    })).expect("tokio works");

    let cs2 = cs.clone();
    rt.block_on(future::poll_fn(move || {
        if cs2.lock().start_broadcast(&InEvent::NextState).is_not_ready() {
            Ok::<_, ()>(Async::NotReady)
        } else {
            Ok(Async::Ready(()))
        }
    })).unwrap();
    let cs_fut = cs.clone();
    rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
        let mut cs = cs_fut.lock();
        if cs.complete_broadcast().is_not_ready() {
            return Ok(Async::NotReady)
        }
        assert_matches!(cs.poll(), Async::Ready(CollectionEvent::NodeEvent{peer: _, event}) => {
            assert_matches!(event, OutEvent::Custom("init"));
        });
        Ok(Async::Ready(()))
    })).expect("tokio works");


    let cs2 = cs.clone();
    rt.block_on(future::poll_fn(move || {
        if cs2.lock().start_broadcast(&InEvent::NextState).is_not_ready() {
            Ok::<_, ()>(Async::NotReady)
        } else {
            Ok(Async::Ready(()))
        }
    })).unwrap();
    let cs_fut = cs.clone();
    rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
        let mut cs = cs_fut.lock();
        if cs.complete_broadcast().is_not_ready() {
            return Ok(Async::NotReady)
        }
        assert_matches!(cs.poll(), Async::Ready(CollectionEvent::NodeEvent{peer: _, event}) => {
            assert_matches!(event, OutEvent::Custom("from handler 1"));
        });
        Ok(Async::Ready(()))
    })).expect("tokio works");

    let cs2 = cs.clone();
    rt.block_on(future::poll_fn(move || {
        if cs2.lock().start_broadcast(&InEvent::NextState).is_not_ready() {
            Ok::<_, ()>(Async::NotReady)
        } else {
            Ok(Async::Ready(()))
        }
    })).unwrap();
    let cs_fut = cs.clone();
    rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
        let mut cs = cs_fut.lock();
        if cs.complete_broadcast().is_not_ready() {
            return Ok(Async::NotReady)
        }
        assert_matches!(cs.poll(), Async::Ready(CollectionEvent::NodeEvent{peer: _, event}) => {
            assert_matches!(event, OutEvent::Custom("from handler 2"));
        });
        Ok(Async::Ready(()))
    })).expect("tokio works");
}

#[test]
fn task_closed_with_error_while_task_is_pending_yields_reach_error() {
    let cs = Arc::new(Mutex::new(TestCollectionStream::new()));
    let task_inner_fut = future::err(std::io::Error::new(std::io::ErrorKind::Other, "inner fut error"));
    let reach_attempt_id = cs.lock().add_reach_attempt(task_inner_fut, Handler::default());

    let mut rt = Builder::new().core_threads(1).build().unwrap();
    let cs_fut = cs.clone();
    rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
        let mut cs = cs_fut.lock();
        assert_matches!(cs.poll(), Async::NotReady);
        Ok(Async::Ready(()))
    })).expect("tokio works");

    let cs_fut = cs.clone();
    rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
        let mut cs = cs_fut.lock();
        assert_matches!(cs.poll(), Async::Ready(collection_ev) => {
            assert_matches!(collection_ev, CollectionEvent::ReachError {id, error, ..} => {
                assert_eq!(id, reach_attempt_id);
                assert_eq!(error.to_string(), "inner fut error");
            });

        });
        Ok(Async::Ready(()))
    })).expect("tokio works");

}

#[test]
fn task_closed_with_error_when_task_is_connected_yields_node_error() {
    let cs = Arc::new(Mutex::new(TestCollectionStream::new()));
    let peer_id = PeerId::random();
    let muxer = DummyMuxer::new();
    let task_inner_fut = future::ok((peer_id.clone(), muxer));
    let mut handler = Handler::default();
    handler.next_states = vec![HandlerState::Err]; // triggered when sending a NextState event

    cs.lock().add_reach_attempt(task_inner_fut, handler);
    let mut rt = Builder::new().core_threads(1).build().unwrap();

    // Kick it off
    let cs2 = cs.clone();
    rt.block_on(future::poll_fn(move || {
        if cs2.lock().start_broadcast(&InEvent::NextState).is_not_ready() {
            Ok::<_, ()>(Async::NotReady)
        } else {
            Ok(Async::Ready(()))
        }
    })).unwrap();
    let cs_fut = cs.clone();
    rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
        let mut cs = cs_fut.lock();
        assert_matches!(cs.poll(), Async::NotReady);
        // send an event so the Handler errors in two polls
        Ok(cs.complete_broadcast())
    })).expect("tokio works");

    // Accept the new node
    let cs_fut = cs.clone();
    rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
        let mut cs = cs_fut.lock();
        // NodeReached, accept the connection so the task transitions from Pending to Connected
        assert_matches!(cs.poll(), Async::Ready(CollectionEvent::NodeReached(reach_ev)) => {
            reach_ev.accept(());
        });
        Ok(Async::Ready(()))
    })).expect("tokio works");

    assert!(cs.lock().has_connection(&peer_id));

    // Assert the node errored
    let cs_fut = cs.clone();
    rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
        let mut cs = cs_fut.lock();
        assert_matches!(cs.poll(), Async::Ready(collection_ev) => {
            assert_matches!(collection_ev, CollectionEvent::NodeClosed{..});
        });
        Ok(Async::Ready(()))
    })).expect("tokio works");
}

#[test]
fn interrupting_a_pending_connection_attempt_is_ok() {
    let mut cs = TestCollectionStream::new();
    let fut = future::empty();
    let reach_id = cs.add_reach_attempt(fut, Handler::default());
    let interrupt = cs.interrupt(reach_id);
    assert!(interrupt.is_ok());
}

#[test]
fn interrupting_a_connection_attempt_twice_is_err() {
    let mut cs = TestCollectionStream::new();
    let fut = future::empty();
    let reach_id = cs.add_reach_attempt(fut, Handler::default());
    assert!(cs.interrupt(reach_id).is_ok());
    assert_matches!(cs.interrupt(reach_id), Err(InterruptError::ReachAttemptNotFound))
}

#[test]
fn interrupting_an_established_connection_is_err() {
    let cs = Arc::new(Mutex::new(TestCollectionStream::new()));
    let peer_id = PeerId::random();
    let muxer = DummyMuxer::new();
    let task_inner_fut = future::ok((peer_id.clone(), muxer));
    let handler = Handler::default();

    let reach_id = cs.lock().add_reach_attempt(task_inner_fut, handler);
    let mut rt = Builder::new().core_threads(1).build().unwrap();

    // Kick it off
    let cs_fut = cs.clone();
    rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
        let mut cs = cs_fut.lock();
        assert_matches!(cs.poll(), Async::NotReady);
        // send an event so the Handler errors in two polls
        Ok(Async::Ready(()))
    })).expect("tokio works");

    // Accept the new node
    let cs_fut = cs.clone();
    rt.block_on(future::poll_fn(move || -> Poll<_, ()> {
        let mut cs = cs_fut.lock();
        // NodeReached, accept the connection so the task transitions from Pending to Connected
        assert_matches!(cs.poll(), Async::Ready(CollectionEvent::NodeReached(reach_ev)) => {
            reach_ev.accept(());
        });
        Ok(Async::Ready(()))
    })).expect("tokio works");

    assert!(cs.lock().has_connection(&peer_id), "Connection was not established");

    assert_matches!(cs.lock().interrupt(reach_id), Err(InterruptError::AlreadyReached));
}
