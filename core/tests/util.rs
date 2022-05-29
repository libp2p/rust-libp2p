#![allow(dead_code)]

use futures::prelude::*;
use libp2p_core::muxing::StreamMuxer;
use std::{pin::Pin, task::Context, task::Poll};

pub struct CloseMuxer<M> {
    state: CloseMuxerState<M>,
}

impl<M> CloseMuxer<M> {
    pub fn new(m: M) -> CloseMuxer<M> {
        CloseMuxer {
            state: CloseMuxerState::Close(m),
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
    M::Error: From<std::io::Error>,
{
    type Output = Result<M, M::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match std::mem::replace(&mut self.state, CloseMuxerState::Done) {
                CloseMuxerState::Close(muxer) => {
                    if !muxer.poll_close(cx)?.is_ready() {
                        self.state = CloseMuxerState::Close(muxer);
                        return Poll::Pending;
                    }
                    return Poll::Ready(Ok(muxer));
                }
                CloseMuxerState::Done => panic!(),
            }
        }
    }
}

impl<M> Unpin for CloseMuxer<M> {}
