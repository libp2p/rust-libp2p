use libp2p_swarm::{ProtocolsHandlerEvent, SubstreamProtocol};
use std::mem;
use std::task::{Context, Poll};

#[derive(Debug)]
pub enum SubstreamState<S> {
    /// There is no substream.
    None,
    /// The substream is in an active state.
    Active(S),
    /// Something went seriously wrong.
    Poisoned,
}

/// Advances a substream state machine.
///
///
pub trait Advance<'handler>: Sized {
    type Event;
    type Params;
    type Error;
    type Protocol;

    fn advance(
        self,
        cx: &mut Context<'_>,
        params: &mut Self::Params,
    ) -> Result<Next<Self, Self::Event, Self::Protocol>, Self::Error>;
}

/// Defines the results of advancing a state machine.
pub enum Next<TState, TEvent, TProtocol> {
    /// Return from the `poll` function to emit `event`. Set the state machine to `next_state`.
    EmitEvent { event: TEvent, next_state: TState },
    /// Return from the `poll` function because we cannot do any more work. Set the state machine to `next_state`.
    Pending { next_state: TState },
    /// Return from the `poll` function to open a new substream. Set the state machine to `next_state`.
    OpenSubstream {
        protocol: TProtocol,
        next_state: TState,
    },
    /// Continue with advancing the state machine.
    Continue { next_state: TState },
    /// The state machine finished.
    Done,
}

impl<TState> SubstreamState<TState> {
    pub fn poll<'handler, TEvent, TUpgrade, TInfo, TError>(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut TState::Params,
    ) -> Poll<ProtocolsHandlerEvent<TUpgrade, TInfo, TEvent, TError>>
    where
        TState: Advance<
            'handler,
            Event = TEvent,
            Protocol = SubstreamProtocol<TUpgrade, TInfo>,
            Error = TError,
        >,
    {
        loop {
            let state = match mem::replace(self, SubstreamState::Poisoned) {
                SubstreamState::None => {
                    *self = SubstreamState::None;
                    return Poll::Pending;
                }
                SubstreamState::Active(state) => state,
                SubstreamState::Poisoned => {
                    unreachable!("reached poisoned state")
                }
            };

            match state.advance(cx, params) {
                Ok(Next::Continue { next_state }) => {
                    *self = SubstreamState::Active(next_state);
                    continue;
                }
                Ok(Next::EmitEvent { event, next_state }) => {
                    *self = SubstreamState::Active(next_state);
                    return Poll::Ready(ProtocolsHandlerEvent::Custom(event));
                }
                Ok(Next::Pending { next_state }) => {
                    *self = SubstreamState::Active(next_state);
                    return Poll::Pending;
                }
                Ok(Next::OpenSubstream {
                    protocol,
                    next_state,
                }) => {
                    *self = SubstreamState::Active(next_state);
                    return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                        protocol,
                    });
                }
                Ok(Next::Done) => {
                    *self = SubstreamState::None;
                    return Poll::Pending;
                }
                Err(e) => return Poll::Ready(ProtocolsHandlerEvent::Close(e)),
            }
        }
    }
}
