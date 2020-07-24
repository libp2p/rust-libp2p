
#![allow(dead_code)]

use futures::prelude::*;
use libp2p_core::{
    Multiaddr,
    connection::{
        ConnectionHandler,
        ConnectionHandlerEvent,
        Substream,
        SubstreamEndpoint,
    },
    muxing::{StreamMuxer, StreamMuxerBox},
};
use std::{io, pin::Pin, task::Context, task::Poll};

pub struct TestHandler();

impl ConnectionHandler for TestHandler {
    type InEvent = ();
    type OutEvent = ();
    type Error = io::Error;
    type Substream = Substream<StreamMuxerBox>;
    type OutboundOpenInfo = ();

    fn inject_substream(&mut self, _: Self::Substream, _: SubstreamEndpoint<Self::OutboundOpenInfo>)
    {}

    fn inject_event(&mut self, _: Self::InEvent)
    {}

    fn inject_address_change(&mut self, _: &Multiaddr)
    {}

    fn poll(&mut self, _: &mut Context)
        -> Poll<Result<ConnectionHandlerEvent<Self::OutboundOpenInfo, Self::OutEvent>, Self::Error>>
    {
        Poll::Ready(Ok(ConnectionHandlerEvent::Custom(())))
    }
}

pub struct CloseMuxer<M> {
    state: CloseMuxerState<M>,
}

impl<M> CloseMuxer<M> {
    pub fn new(m: M) -> CloseMuxer<M> {
        CloseMuxer {
            state: CloseMuxerState::Close(m)
        }
    }
}

pub enum CloseMuxerState<M> {
    Close(M),
    Done,
}

impl<M> Future for CloseMuxer<M>
where
    M: StreamMuxer,
    M::Error: From<std::io::Error>
{
    type Output = Result<M, M::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        loop {
            match std::mem::replace(&mut self.state, CloseMuxerState::Done) {
                CloseMuxerState::Close(muxer) => {
                    if !muxer.close(cx)?.is_ready() {
                        self.state = CloseMuxerState::Close(muxer);
                        return Poll::Pending
                    }
                    return Poll::Ready(Ok(muxer))
                }
                CloseMuxerState::Done => panic!()
            }
        }
    }
}

impl<M> Unpin for CloseMuxer<M> {
}
