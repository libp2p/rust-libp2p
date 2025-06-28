// Copyright 2022 Parity Technologies (UK) Ltd.
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

use std::io;

use bytes::Bytes;

use crate::proto::Flag;

#[derive(Debug, Copy, Clone)]
pub(crate) enum State {
    Open,
    ReadClosed,
    WriteClosed,
    ClosingRead {
        /// Whether the write side of our channel was already closed.
        write_closed: bool,
        inner: Closing,
    },
    ClosingWrite {
        /// Whether the read side of our channel was already closed.
        read_closed: bool,
        inner: Closing,
    },
    /// We've received a FIN and need to send a FIN_ACK
    ReadClosedNeedFinAck {
        /// Whether the write side of our channel was already closed.
        write_closed: bool,
    },
    /// We've sent a FIN and are waiting for a FIN_ACK
    WriteSentFinWaitingForAck {
        /// Whether the read side of our channel was already closed.
        read_closed: bool,
    },
    BothClosed {
        reset: bool,
    },
}

/// Represents the state of closing one half (either read or write) of the connection.
///
/// Gracefully closing the read or write requires sending the `STOP_SENDING` or `FIN` flag
/// respectively and flushing the underlying connection.
#[derive(Debug, Copy, Clone)]
pub(crate) enum Closing {
    Requested,
    MessageSent,
}

impl State {
    /// Performs a state transition for a flag contained in an inbound message.
    pub(crate) fn handle_inbound_flag(&mut self, flag: Flag, buffer: &mut Bytes) {
        let current = *self;

        match (current, flag) {
            (Self::Open, Flag::FIN) => {
                *self = Self::ReadClosedNeedFinAck {
                    write_closed: false,
                };
            }
            (Self::WriteClosed, Flag::FIN) => {
                *self = Self::ReadClosedNeedFinAck { write_closed: true };
            }
            (Self::WriteSentFinWaitingForAck { read_closed: false }, Flag::FIN) => {
                *self = Self::ReadClosedNeedFinAck {
                    write_closed: false,
                };
            }
            (Self::WriteSentFinWaitingForAck { read_closed: true }, Flag::FIN) => {
                *self = Self::BothClosed { reset: false };
            }
            (Self::Open, Flag::STOP_SENDING) => {
                *self = Self::WriteClosed;
            }
            (Self::ReadClosed, Flag::STOP_SENDING) => {
                *self = Self::BothClosed { reset: false };
            }
            (
                Self::ReadClosedNeedFinAck {
                    write_closed: false,
                },
                Flag::STOP_SENDING,
            ) => {
                *self = Self::ReadClosedNeedFinAck { write_closed: true };
            }
            (Self::ReadClosedNeedFinAck { write_closed: true }, Flag::STOP_SENDING) => {
                // Already closed, ignore
            }
            (Self::WriteSentFinWaitingForAck { read_closed: _ }, Flag::STOP_SENDING) => {
                *self = Self::WriteSentFinWaitingForAck { read_closed: true };
            }
            (Self::WriteSentFinWaitingForAck { read_closed: false }, Flag::FIN_ACK) => {
                *self = Self::WriteClosed;
            }
            (Self::WriteSentFinWaitingForAck { read_closed: true }, Flag::FIN_ACK) => {
                *self = Self::BothClosed { reset: false };
            }
            (
                Self::ClosingWrite {
                    read_closed,
                    inner: Closing::MessageSent,
                },
                Flag::FIN_ACK,
            ) => {
                *self = if read_closed {
                    Self::BothClosed { reset: false }
                } else {
                    Self::WriteClosed
                };
            }
            (_, Flag::RESET) => {
                buffer.clear();
                *self = Self::BothClosed { reset: true };
            }
            _ => {}
        }
    }

    pub(crate) fn write_closed(&mut self) {
        match self {
            State::ClosingWrite {
                read_closed: true,
                inner,
            } => {
                debug_assert!(matches!(inner, Closing::MessageSent));

                *self = State::WriteSentFinWaitingForAck { read_closed: true };
            }
            State::ClosingWrite {
                read_closed: false,
                inner,
            } => {
                debug_assert!(matches!(inner, Closing::MessageSent));

                *self = State::WriteSentFinWaitingForAck { read_closed: false };
            }
            State::ReadClosedNeedFinAck { .. } => {
                unreachable!("write_closed called on ReadClosedNeedFinAck state")
            }
            State::WriteSentFinWaitingForAck { .. } => {
                unreachable!("write_closed called on WriteSentFinWaitingForAck state")
            }
            State::Open
            | State::ReadClosed
            | State::WriteClosed
            | State::ClosingRead { .. }
            | State::BothClosed { .. } => {
                unreachable!("bad state machine impl")
            }
        }
    }

    pub(crate) fn close_write_message_sent(&mut self) {
        match self {
            State::ClosingWrite { inner, read_closed } => {
                debug_assert!(matches!(inner, Closing::Requested));

                *self = State::ClosingWrite {
                    read_closed: *read_closed,
                    inner: Closing::MessageSent,
                };
            }
            State::ReadClosedNeedFinAck { .. } => {
                unreachable!("close_write_message_sent called on ReadClosedNeedFinAck state")
            }
            State::WriteSentFinWaitingForAck { .. } => {
                unreachable!("close_write_message_sent called on WriteSentFinWaitingForAck state")
            }
            State::Open
            | State::ReadClosed
            | State::WriteClosed
            | State::ClosingRead { .. }
            | State::BothClosed { .. } => {
                unreachable!("bad state machine impl")
            }
        }
    }

    pub(crate) fn read_closed(&mut self) {
        match self {
            State::ClosingRead {
                write_closed: true,
                inner,
            } => {
                debug_assert!(matches!(inner, Closing::MessageSent));

                *self = State::BothClosed { reset: false };
            }
            State::ClosingRead {
                write_closed: false,
                inner,
            } => {
                debug_assert!(matches!(inner, Closing::MessageSent));

                *self = State::ReadClosed;
            }
            State::ReadClosedNeedFinAck { .. } => {
                unreachable!("read_closed called on ReadClosedNeedFinAck state")
            }
            State::WriteSentFinWaitingForAck { .. } => {
                unreachable!("read_closed called on WriteSentFinWaitingForAck state")
            }
            State::Open
            | State::ReadClosed
            | State::WriteClosed
            | State::ClosingWrite { .. }
            | State::BothClosed { .. } => {
                unreachable!("bad state machine impl")
            }
        }
    }

    pub(crate) fn close_read_message_sent(&mut self) {
        match self {
            State::ClosingRead {
                inner,
                write_closed,
            } => {
                debug_assert!(matches!(inner, Closing::Requested));

                *self = State::ClosingRead {
                    write_closed: *write_closed,
                    inner: Closing::MessageSent,
                };
            }
            State::ReadClosedNeedFinAck { .. } => {
                unreachable!("close_read_message_sent called on ReadClosedNeedFinAck state")
            }
            State::WriteSentFinWaitingForAck { .. } => {
                unreachable!("close_read_message_sent called on WriteSentFinWaitingForAck state")
            }
            State::Open
            | State::ReadClosed
            | State::WriteClosed
            | State::ClosingWrite { .. }
            | State::BothClosed { .. } => {
                unreachable!("bad state machine impl")
            }
        }
    }

    /// Whether we should read from the stream in the [`futures::AsyncWrite`] implementation.
    ///
    /// This is necessary for read-closed streams because we would otherwise
    /// not read any more flags from the socket.
    pub(crate) fn read_flags_in_async_write(&self) -> bool {
        matches!(
            self,
            Self::ReadClosed
                | Self::ReadClosedNeedFinAck { .. }
                | Self::WriteSentFinWaitingForAck { .. }
        )
    }

    /// Acts as a "barrier" for [`futures::AsyncRead::poll_read`].
    pub(crate) fn read_barrier(&self) -> io::Result<()> {
        use State::*;

        let kind = match self {
            Open
            | WriteClosed
            | ClosingWrite {
                read_closed: false, ..
            }
            | WriteSentFinWaitingForAck { read_closed: false } => return Ok(()),
            ClosingWrite {
                read_closed: true, ..
            }
            | ReadClosed
            | ReadClosedNeedFinAck { .. }
            | WriteSentFinWaitingForAck { read_closed: true }
            | ClosingRead { .. }
            | BothClosed { reset: false } => io::ErrorKind::BrokenPipe,
            BothClosed { reset: true } => io::ErrorKind::ConnectionReset,
        };

        Err(kind.into())
    }

    /// Acts as a "barrier" for [`futures::AsyncWrite::poll_write`].
    pub(crate) fn write_barrier(&self) -> io::Result<()> {
        use State::*;

        let kind = match self {
            Open
            | ReadClosed
            | ReadClosedNeedFinAck {
                write_closed: false,
            }
            | ClosingRead {
                write_closed: false,
                ..
            } => return Ok(()),
            ClosingRead {
                write_closed: true, ..
            }
            | WriteClosed
            | ReadClosedNeedFinAck { write_closed: true }
            | WriteSentFinWaitingForAck { .. }
            | ClosingWrite { .. }
            | BothClosed { reset: false } => io::ErrorKind::BrokenPipe,
            BothClosed { reset: true } => io::ErrorKind::ConnectionReset,
        };

        Err(kind.into())
    }

    /// Acts as a "barrier" for [`futures::AsyncWrite::poll_close`].
    pub(crate) fn close_write_barrier(&mut self) -> io::Result<Option<Closing>> {
        loop {
            match &self {
                State::WriteClosed => return Ok(None),

                State::ClosingWrite { inner, .. } => return Ok(Some(*inner)),

                State::WriteSentFinWaitingForAck { .. } => {
                    return Err(io::Error::other("waiting for FIN_ACK before closing"))
                }

                State::Open => {
                    *self = Self::ClosingWrite {
                        read_closed: false,
                        inner: Closing::Requested,
                    };
                }
                State::ReadClosed => {
                    *self = Self::ClosingWrite {
                        read_closed: true,
                        inner: Closing::Requested,
                    };
                }
                State::ReadClosedNeedFinAck {
                    write_closed: false,
                } => {
                    *self = Self::ClosingWrite {
                        read_closed: true,
                        inner: Closing::Requested,
                    };
                }
                State::ReadClosedNeedFinAck { write_closed: true } => {
                    return Err(io::ErrorKind::BrokenPipe.into())
                }

                State::ClosingRead {
                    write_closed: true, ..
                }
                | State::BothClosed { reset: false } => {
                    return Err(io::ErrorKind::BrokenPipe.into())
                }

                State::ClosingRead {
                    write_closed: false,
                    ..
                } => {
                    return Err(io::Error::other(
                        "cannot close read half while closing write half",
                    ))
                }

                State::BothClosed { reset: true } => {
                    return Err(io::ErrorKind::ConnectionReset.into())
                }
            }
        }
    }

    /// Acts as a "barrier" for [`Stream::poll_close_read`](super::Stream::poll_close_read).
    pub(crate) fn close_read_barrier(&mut self) -> io::Result<Option<Closing>> {
        loop {
            match self {
                State::ReadClosed => return Ok(None),

                State::ClosingRead { inner, .. } => return Ok(Some(*inner)),

                State::ReadClosedNeedFinAck { .. } => return Ok(None),

                State::Open => {
                    *self = Self::ClosingRead {
                        write_closed: false,
                        inner: Closing::Requested,
                    };
                }
                State::WriteClosed => {
                    *self = Self::ClosingRead {
                        write_closed: true,
                        inner: Closing::Requested,
                    };
                }
                State::WriteSentFinWaitingForAck { read_closed: false } => {
                    *self = Self::ClosingRead {
                        write_closed: false,
                        inner: Closing::Requested,
                    };
                }
                State::WriteSentFinWaitingForAck { read_closed: true } => {
                    return Err(io::ErrorKind::BrokenPipe.into())
                }

                State::ClosingWrite {
                    read_closed: true, ..
                }
                | State::BothClosed { reset: false } => {
                    return Err(io::ErrorKind::BrokenPipe.into())
                }

                State::ClosingWrite {
                    read_closed: false, ..
                } => {
                    return Err(io::Error::other(
                        "cannot close write half while closing read half",
                    ))
                }

                State::BothClosed { reset: true } => {
                    return Err(io::ErrorKind::ConnectionReset.into())
                }
            }
        }
    }

    /// Returns whether the state requires sending a FIN_ACK.
    /// This should be called by the stream implementation to check if it needs to send a FIN_ACK.
    pub(crate) fn needs_fin_ack(&self) -> bool {
        matches!(self, Self::ReadClosedNeedFinAck { .. })
    }

    /// Marks that a FIN_ACK has been sent in response to a received FIN.
    /// This transitions from ReadClosedNeedFinAck to ReadClosed.
    pub(crate) fn fin_ack_sent(&mut self) {
        match self {
            State::ReadClosedNeedFinAck {
                write_closed: false,
            } => {
                *self = State::ReadClosed;
            }
            State::ReadClosedNeedFinAck { write_closed: true } => {
                *self = State::BothClosed { reset: false };
            }
            _ => {
                unreachable!("fin_ack_sent called on wrong state")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    use super::*;

    #[test]
    fn cannot_read_after_receiving_fin() {
        let mut open = State::Open;

        open.handle_inbound_flag(Flag::FIN, &mut Bytes::default());
        // After receiving FIN, we're in ReadClosedNeedFinAck state but read barrier should still
        // prevent reading
        let error = open.read_barrier().unwrap_err();

        assert_eq!(error.kind(), ErrorKind::BrokenPipe)
    }

    #[test]
    fn cannot_read_after_closing_read() {
        let mut open = State::Open;

        open.close_read_barrier().unwrap();
        open.close_read_message_sent();
        open.read_closed();
        let error = open.read_barrier().unwrap_err();

        assert_eq!(error.kind(), ErrorKind::BrokenPipe)
    }

    #[test]
    fn cannot_write_after_receiving_stop_sending() {
        let mut open = State::Open;

        open.handle_inbound_flag(Flag::STOP_SENDING, &mut Bytes::default());
        let error = open.write_barrier().unwrap_err();

        assert_eq!(error.kind(), ErrorKind::BrokenPipe)
    }

    #[test]
    fn cannot_write_after_closing_write() {
        let mut open = State::Open;

        open.close_write_barrier().unwrap();
        open.close_write_message_sent();
        open.write_closed();
        let error = open.write_barrier().unwrap_err();

        assert_eq!(error.kind(), ErrorKind::BrokenPipe)
    }

    #[test]
    fn everything_broken_after_receiving_reset() {
        let mut open = State::Open;

        open.handle_inbound_flag(Flag::RESET, &mut Bytes::default());
        let error1 = open.read_barrier().unwrap_err();
        let error2 = open.write_barrier().unwrap_err();
        let error3 = open.close_write_barrier().unwrap_err();
        let error4 = open.close_read_barrier().unwrap_err();

        assert_eq!(error1.kind(), ErrorKind::ConnectionReset);
        assert_eq!(error2.kind(), ErrorKind::ConnectionReset);
        assert_eq!(error3.kind(), ErrorKind::ConnectionReset);
        assert_eq!(error4.kind(), ErrorKind::ConnectionReset);
    }

    #[test]
    fn should_read_flags_in_async_write_after_read_closed() {
        let mut open = State::Open;

        open.handle_inbound_flag(Flag::FIN, &mut Bytes::default());

        assert!(open.read_flags_in_async_write())
    }

    #[test]
    fn cannot_read_or_write_after_receiving_fin_and_stop_sending() {
        let mut open = State::Open;

        open.handle_inbound_flag(Flag::FIN, &mut Bytes::default());
        open.handle_inbound_flag(Flag::STOP_SENDING, &mut Bytes::default());

        let error1 = open.read_barrier().unwrap_err();
        let error2 = open.write_barrier().unwrap_err();

        assert_eq!(error1.kind(), ErrorKind::BrokenPipe);
        assert_eq!(error2.kind(), ErrorKind::BrokenPipe);
    }

    #[test]
    fn can_read_after_closing_write() {
        let mut open = State::Open;

        open.close_write_barrier().unwrap();
        open.close_write_message_sent();
        open.write_closed();

        open.read_barrier().unwrap();
    }

    #[test]
    fn can_write_after_closing_read() {
        let mut open = State::Open;

        open.close_read_barrier().unwrap();
        open.close_read_message_sent();
        open.read_closed();

        open.write_barrier().unwrap();
    }

    #[test]
    fn cannot_write_after_starting_close() {
        let mut open = State::Open;

        open.close_write_barrier().expect("to close in open");
        let error = open.write_barrier().unwrap_err();

        assert_eq!(error.kind(), ErrorKind::BrokenPipe);
    }

    #[test]
    fn cannot_read_after_starting_close() {
        let mut open = State::Open;

        open.close_read_barrier().expect("to close in open");
        let error = open.read_barrier().unwrap_err();

        assert_eq!(error.kind(), ErrorKind::BrokenPipe);
    }

    #[test]
    fn can_read_in_open() {
        let open = State::Open;

        let result = open.read_barrier();

        result.unwrap();
    }

    #[test]
    fn can_write_in_open() {
        let open = State::Open;

        let result = open.write_barrier();

        result.unwrap();
    }

    #[test]
    fn write_close_barrier_returns_ok_when_closed() {
        let mut open = State::Open;

        open.close_write_barrier().unwrap();
        open.close_write_message_sent();
        open.write_closed();

        // After write_closed(), we're waiting for FIN_ACK, so close_write_barrier should return
        // error
        let result = open.close_write_barrier();
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "waiting for FIN_ACK before closing"
        );

        // After receiving FIN_ACK, we should be in WriteClosed state
        open.handle_inbound_flag(Flag::FIN_ACK, &mut Bytes::default());
        let maybe = open.close_write_barrier().unwrap();
        assert!(maybe.is_none())
    }

    #[test]
    fn read_close_barrier_returns_ok_when_closed() {
        let mut open = State::Open;

        open.close_read_barrier().unwrap();
        open.close_read_message_sent();
        open.read_closed();

        let maybe = open.close_read_barrier().unwrap();

        assert!(maybe.is_none())
    }

    #[test]
    fn reset_flag_clears_buffer() {
        let mut open = State::Open;
        let mut buffer = Bytes::copy_from_slice(b"foobar");

        open.handle_inbound_flag(Flag::RESET, &mut buffer);

        assert!(buffer.is_empty());
    }

    #[test]
    fn fin_requires_fin_ack() {
        let mut open = State::Open;

        open.handle_inbound_flag(Flag::FIN, &mut Bytes::default());

        assert!(open.needs_fin_ack());
        assert!(matches!(
            open,
            State::ReadClosedNeedFinAck {
                write_closed: false
            }
        ));
    }

    #[test]
    fn fin_ack_sent_transitions_to_read_closed() {
        let mut state = State::ReadClosedNeedFinAck {
            write_closed: false,
        };

        state.fin_ack_sent();

        assert!(!state.needs_fin_ack());
        assert!(matches!(state, State::ReadClosed));
    }

    #[test]
    fn fin_ack_sent_with_write_closed_transitions_to_both_closed() {
        let mut state = State::ReadClosedNeedFinAck { write_closed: true };

        state.fin_ack_sent();

        assert!(!state.needs_fin_ack());
        assert!(matches!(state, State::BothClosed { reset: false }));
    }

    #[test]
    fn fin_ack_completes_write_close() {
        let mut state = State::WriteSentFinWaitingForAck { read_closed: false };

        state.handle_inbound_flag(Flag::FIN_ACK, &mut Bytes::default());

        assert!(matches!(state, State::WriteClosed));
    }

    #[test]
    fn fin_ack_with_read_closed_transitions_to_both_closed() {
        let mut state = State::WriteSentFinWaitingForAck { read_closed: true };

        state.handle_inbound_flag(Flag::FIN_ACK, &mut Bytes::default());

        assert!(matches!(state, State::BothClosed { reset: false }));
    }

    #[test]
    fn simultaneous_fin_exchange() {
        let mut state = State::WriteSentFinWaitingForAck { read_closed: false };

        // Receive FIN while waiting for FIN_ACK
        state.handle_inbound_flag(Flag::FIN, &mut Bytes::default());

        assert!(state.needs_fin_ack());
        assert!(matches!(
            state,
            State::ReadClosedNeedFinAck {
                write_closed: false
            }
        ));

        // Send FIN_ACK
        state.fin_ack_sent();

        assert!(matches!(state, State::ReadClosed));
    }

    #[test]
    fn write_close_waits_for_fin_ack() {
        let mut state = State::WriteSentFinWaitingForAck { read_closed: false };

        let result = state.close_write_barrier();

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "waiting for FIN_ACK before closing"
        );
    }

    #[test]
    fn read_flags_in_async_write_with_fin_ack_states() {
        let state_need_ack = State::ReadClosedNeedFinAck {
            write_closed: false,
        };
        let state_waiting_ack = State::WriteSentFinWaitingForAck { read_closed: false };

        assert!(state_need_ack.read_flags_in_async_write());
        assert!(state_waiting_ack.read_flags_in_async_write());
    }

    #[test]
    fn complete_fin_ack_handshake_example() {
        // This test demonstrates the complete FIN_ACK handshake as described in the spec:
        // NodeA closes for writing, NodeB delays allowing the channel to close until it
        // also finishes writing.

        let mut node_a = State::Open;
        let mut node_b = State::Open;

        // NodeA wants to close for writing
        node_a.close_write_barrier().unwrap();
        node_a.close_write_message_sent();
        node_a.write_closed();

        // NodeA is now waiting for FIN_ACK
        assert!(matches!(
            node_a,
            State::WriteSentFinWaitingForAck { read_closed: false }
        ));

        // NodeB receives the FIN from NodeA
        node_b.handle_inbound_flag(Flag::FIN, &mut Bytes::default());

        // NodeB should now need to send a FIN_ACK
        assert!(node_b.needs_fin_ack());
        assert!(matches!(
            node_b,
            State::ReadClosedNeedFinAck {
                write_closed: false
            }
        ));

        // NodeB sends FIN_ACK (simulated by calling fin_ack_sent)
        node_b.fin_ack_sent();
        assert!(matches!(node_b, State::ReadClosed));

        // NodeA receives the FIN_ACK
        node_a.handle_inbound_flag(Flag::FIN_ACK, &mut Bytes::default());

        // NodeA's write side is now closed
        assert!(matches!(node_a, State::WriteClosed));

        // NodeB also wants to close for writing
        node_b.close_write_barrier().unwrap();
        node_b.close_write_message_sent();
        node_b.write_closed();

        // NodeB is now waiting for FIN_ACK
        assert!(matches!(
            node_b,
            State::WriteSentFinWaitingForAck { read_closed: true }
        ));

        // NodeA receives the FIN from NodeB
        node_a.handle_inbound_flag(Flag::FIN, &mut Bytes::default());

        // NodeA should now need to send a FIN_ACK
        assert!(node_a.needs_fin_ack());
        assert!(matches!(
            node_a,
            State::ReadClosedNeedFinAck { write_closed: true }
        ));

        // NodeA sends FIN_ACK
        node_a.fin_ack_sent();
        assert!(matches!(node_a, State::BothClosed { reset: false }));

        // NodeB receives the FIN_ACK
        node_b.handle_inbound_flag(Flag::FIN_ACK, &mut Bytes::default());

        // NodeB is now fully closed
        assert!(matches!(node_b, State::BothClosed { reset: false }));

        // Both nodes have successfully closed the channel without data loss
    }
}
