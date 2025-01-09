use std::{
    collections::VecDeque,
    convert::Infallible,
    io,
    iter::{once, repeat},
    task::{Context, Poll},
    time::Duration,
};

use futures::{channel::oneshot, AsyncWrite};
use futures_bounded::FuturesMap;
use libp2p_core::{
    upgrade::{DeniedUpgrade, ReadyUpgrade},
    Multiaddr,
};
use libp2p_swarm::{
    handler::{
        ConnectionEvent, DialUpgradeError, FullyNegotiatedOutbound, OutboundUpgradeSend,
        ProtocolsChange,
    },
    ConnectionHandler, ConnectionHandlerEvent, Stream, StreamProtocol, StreamUpgradeError,
    SubstreamProtocol,
};

use crate::v2::{
    generated::structs::{mod_DialResponse::ResponseStatus, DialStatus},
    protocol::{
        Coder, DialDataRequest, DialDataResponse, DialRequest, Response,
        DATA_FIELD_LEN_UPPER_BOUND, DATA_LEN_LOWER_BOUND, DATA_LEN_UPPER_BOUND,
    },
    Nonce, DIAL_REQUEST_PROTOCOL,
};

#[derive(Debug)]
pub enum ToBehaviour {
    TestOutcome {
        nonce: Nonce,
        outcome: Result<(Multiaddr, usize), Error>,
    },
    PeerHasServerSupport,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Address is not reachable: {error}")]
    AddressNotReachable {
        address: Multiaddr,
        bytes_sent: usize,
        error: DialBackError,
    },
    #[error("Peer does not support AutoNAT dial-request protocol")]
    UnsupportedProtocol,
    #[error("IO error: {0}")]
    Io(io::Error),
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum DialBackError {
    #[error("server failed to establish a connection")]
    NoConnection,
    #[error("dial back stream failed")]
    StreamFailed,
}

pub struct Handler {
    queued_events: VecDeque<
        ConnectionHandlerEvent<
            <Self as ConnectionHandler>::OutboundProtocol,
            (),
            <Self as ConnectionHandler>::ToBehaviour,
        >,
    >,
    outbound: FuturesMap<Nonce, Result<(Multiaddr, usize), Error>>,
    queued_streams: VecDeque<
        oneshot::Sender<
            Result<
                Stream,
                StreamUpgradeError<<ReadyUpgrade<StreamProtocol> as OutboundUpgradeSend>::Error>,
            >,
        >,
    >,
}

impl Handler {
    pub(crate) fn new() -> Self {
        Self {
            queued_events: VecDeque::new(),
            outbound: FuturesMap::new(Duration::from_secs(10), 10),
            queued_streams: VecDeque::default(),
        }
    }

    fn perform_request(&mut self, req: DialRequest) {
        let (tx, rx) = oneshot::channel();
        self.queued_streams.push_back(tx);
        self.queued_events
            .push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(ReadyUpgrade::new(DIAL_REQUEST_PROTOCOL), ()),
            });
        if self
            .outbound
            .try_push(req.nonce, start_stream_handle(req, rx))
            .is_err()
        {
            tracing::debug!("Dial request dropped, too many requests in flight");
        }
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = DialRequest;
    type ToBehaviour = ToBehaviour;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = ReadyUpgrade<StreamProtocol>;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ConnectionHandlerEvent<Self::OutboundProtocol, (), Self::ToBehaviour>> {
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }

        match self.outbound.poll_unpin(cx) {
            Poll::Ready((nonce, Ok(outcome))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    ToBehaviour::TestOutcome { nonce, outcome },
                ))
            }
            Poll::Ready((nonce, Err(_))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    ToBehaviour::TestOutcome {
                        nonce,
                        outcome: Err(Error::Io(io::ErrorKind::TimedOut.into())),
                    },
                ));
            }
            Poll::Pending => {}
        }

        Poll::Pending
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        self.perform_request(event);
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<Self::InboundProtocol, Self::OutboundProtocol>,
    ) {
        match event {
            ConnectionEvent::DialUpgradeError(DialUpgradeError { error, .. }) => {
                tracing::debug!("Dial request failed: {}", error);
                match self.queued_streams.pop_front() {
                    Some(stream_tx) => {
                        let _ = stream_tx.send(Err(error));
                    }
                    None => {
                        tracing::warn!(
                            "Opened unexpected substream without a pending dial request"
                        );
                    }
                }
            }
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol, ..
            }) => match self.queued_streams.pop_front() {
                Some(stream_tx) => {
                    if stream_tx.send(Ok(protocol)).is_err() {
                        tracing::debug!("Failed to send stream to dead handler");
                    }
                }
                None => {
                    tracing::warn!("Opened unexpected substream without a pending dial request");
                }
            },
            ConnectionEvent::RemoteProtocolsChange(ProtocolsChange::Added(mut added)) => {
                if added.any(|p| p.as_ref() == DIAL_REQUEST_PROTOCOL) {
                    self.queued_events
                        .push_back(ConnectionHandlerEvent::NotifyBehaviour(
                            ToBehaviour::PeerHasServerSupport,
                        ));
                }
            }
            _ => {}
        }
    }
}

async fn start_stream_handle(
    req: DialRequest,
    stream_recv: oneshot::Receiver<Result<Stream, StreamUpgradeError<Infallible>>>,
) -> Result<(Multiaddr, usize), Error> {
    let stream = stream_recv
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?
        .map_err(|e| match e {
            StreamUpgradeError::NegotiationFailed => Error::UnsupportedProtocol,
            StreamUpgradeError::Timeout => Error::Io(io::ErrorKind::TimedOut.into()),
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            StreamUpgradeError::Apply(v) => libp2p_core::util::unreachable(v),
            StreamUpgradeError::Io(e) => Error::Io(e),
        })?;

    let mut coder = Coder::new(stream);
    coder.send(req.clone()).await?;

    let (res, bytes_sent) = match coder.next().await? {
        Response::Data(DialDataRequest {
            addr_idx,
            num_bytes,
        }) => {
            if addr_idx >= req.addrs.len() {
                return Err(Error::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "address index out of bounds",
                )));
            }
            if !(DATA_LEN_LOWER_BOUND..=DATA_LEN_UPPER_BOUND).contains(&num_bytes) {
                return Err(Error::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "requested bytes out of bounds",
                )));
            }

            send_aap_data(&mut coder, num_bytes).await?;

            let Response::Dial(dial_response) = coder.next().await? else {
                return Err(Error::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "expected message",
                )));
            };

            (dial_response, num_bytes)
        }
        Response::Dial(dial_response) => (dial_response, 0),
    };
    match coder.close().await {
        Ok(_) => {}
        Err(err) => {
            if err.kind() == io::ErrorKind::ConnectionReset {
                // The AutoNAT server may have already closed the stream
                // (this is normal because the probe is finished),
                // in this case we have this error:
                // Err(Custom { kind: ConnectionReset, error: Stopped(0) })
                // so we silently ignore this error
            } else {
                return Err(err.into());
            }
        }
    }

    match res.status {
        ResponseStatus::E_REQUEST_REJECTED => {
            return Err(Error::Io(io::Error::new(
                io::ErrorKind::Other,
                "server rejected request",
            )))
        }
        ResponseStatus::E_DIAL_REFUSED => {
            return Err(Error::Io(io::Error::new(
                io::ErrorKind::Other,
                "server refused dial",
            )))
        }
        ResponseStatus::E_INTERNAL_ERROR => {
            return Err(Error::Io(io::Error::new(
                io::ErrorKind::Other,
                "server encountered internal error",
            )))
        }
        ResponseStatus::OK => {}
    }

    let tested_address = req
        .addrs
        .get(res.addr_idx)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "address index out of bounds"))?
        .clone();

    match res.dial_status {
        DialStatus::UNUSED => {
            return Err(Error::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unexpected message",
            )))
        }
        DialStatus::E_DIAL_ERROR => {
            return Err(Error::AddressNotReachable {
                address: tested_address,
                bytes_sent,
                error: DialBackError::NoConnection,
            })
        }
        DialStatus::E_DIAL_BACK_ERROR => {
            return Err(Error::AddressNotReachable {
                address: tested_address,
                bytes_sent,
                error: DialBackError::StreamFailed,
            })
        }
        DialStatus::OK => {}
    }

    Ok((tested_address, bytes_sent))
}

async fn send_aap_data<I>(stream: &mut Coder<I>, num_bytes: usize) -> io::Result<()>
where
    I: AsyncWrite + Unpin,
{
    let count_full = num_bytes / DATA_FIELD_LEN_UPPER_BOUND;
    let partial_len = num_bytes % DATA_FIELD_LEN_UPPER_BOUND;
    for req in repeat(DATA_FIELD_LEN_UPPER_BOUND)
        .take(count_full)
        .chain(once(partial_len))
        .filter(|e| *e > 0)
        .map(|data_count| {
            DialDataResponse::new(data_count).expect("data count is unexpectedly too big")
        })
    {
        stream.send(req).await?;
    }

    Ok(())
}
