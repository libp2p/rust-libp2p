
#![allow(dead_code)]

use futures::prelude::*;
use libp2p_core::muxing::StreamMuxer;

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
    type Item = M;
    type Error = M::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match std::mem::replace(&mut self.state, CloseMuxerState::Done) {
                CloseMuxerState::Close(muxer) => {
                    if muxer.close()?.is_not_ready() {
                        self.state = CloseMuxerState::Close(muxer);
                        return Ok(Async::NotReady)
                    }
                    return Ok(Async::Ready(muxer))
                }
                CloseMuxerState::Done => panic!()
            }
        }
    }
}

