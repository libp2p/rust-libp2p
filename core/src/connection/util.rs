use std::{io, task::{Poll, Context}};

use multiaddr::Multiaddr;

use crate::muxing::StreamMuxerBox;

use super::{ConnectionHandler, Substream, SubstreamEndpoint, ConnectionHandlerEvent};


#[derive(Debug)]
pub struct TestHandler();

impl ConnectionHandler for TestHandler {
    type InEvent = ();
    type OutEvent = ();
    type Error = io::Error;
    type Substream = Substream<StreamMuxerBox>;
    type OutboundOpenInfo = ();

    fn inject_substream(
        &mut self,
        _: Self::Substream,
        _: SubstreamEndpoint<Self::OutboundOpenInfo>,
    ) {
    }

    fn inject_event(&mut self, _: Self::InEvent) {}

    fn inject_address_change(&mut self, _: &Multiaddr) {}

    fn poll(
        &mut self,
        _: &mut Context<'_>,
    ) -> Poll<Result<ConnectionHandlerEvent<Self::OutboundOpenInfo, Self::OutEvent>, Self::Error>>
    {
        Poll::Pending
    }
}