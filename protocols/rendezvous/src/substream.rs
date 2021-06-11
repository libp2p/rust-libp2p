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

/// Advances a state machine.
pub trait Advance: Sized {
    type Event;

    fn advance(self, cx: &mut Context<'_>) -> Next<Self, Self::Event>;
}

/// Defines the results of advancing a state machine.
pub enum Next<S, E> {
    /// Return from the `poll` function, either because are `Ready` or there is no more work to do (`Pending`).
    Return { poll: Poll<E>, next_state: S },
    /// Continue with polling the state.
    Continue { next_state: S },
    /// The state machine finished gracefully.
    Done { event: Option<E> },
}

impl<S, E> SubstreamState<S>
where
    S: Advance<Event = E>,
{
    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<E> {
        loop {
            let next = match mem::replace(self, SubstreamState::Poisoned) {
                SubstreamState::None => {
                    *self = SubstreamState::None;
                    return Poll::Pending;
                }
                SubstreamState::Active(state) => state.advance(cx),
                SubstreamState::Poisoned => {
                    unreachable!("reached poisoned state")
                }
            };

            match next {
                Next::Continue { next_state } => {
                    *self = SubstreamState::Active(next_state);
                    continue;
                }
                Next::Return { poll, next_state } => {
                    *self = SubstreamState::Active(next_state);
                    return poll;
                }
                Next::Done { event: final_event } => {
                    *self = SubstreamState::None;
                    if let Some(final_event) = final_event {
                        return Poll::Ready(final_event);
                    }

                    return Poll::Pending;
                }
            }
        }
    }
}
