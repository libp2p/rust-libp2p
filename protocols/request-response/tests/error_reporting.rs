use async_std::channel;
use async_std::future::timeout;
use async_std::task::{sleep, spawn};
use async_trait::async_trait;
use futures::prelude::*;
use libp2p_request_response as request_response;
use libp2p_request_response::ProtocolSupport;
use libp2p_swarm::{StreamProtocol, Swarm};
use libp2p_swarm_test::SwarmExt;
use request_response::{Codec, OutboundFailure};
use std::time::Duration;
use std::{io, iter};

#[async_std::test]
async fn report_outbound_failure_on_read_response() {
    let _ = env_logger::try_init();

    let protocols = iter::once((StreamProtocol::new("/test/1"), ProtocolSupport::Full));
    let cfg = request_response::Config::default();

    let mut swarm1 = Swarm::new_ephemeral(|_| {
        request_response::Behaviour::<TestCodec>::new(protocols.clone(), cfg.clone())
    });
    let peer1_id = *swarm1.local_peer_id();

    let mut swarm2 = Swarm::new_ephemeral(|_| {
        request_response::Behaviour::<TestCodec>::new(protocols.clone(), cfg.clone())
    });
    let peer2_id = *swarm2.local_peer_id();

    swarm1.listen().await;
    swarm2.connect(&mut swarm1).await;

    // Expects Action::FailOnReadResponse, replies with Action::FailOnReadResponse
    let swarm1_task = async move {
        loop {
            match swarm1.select_next_some().await.try_into_behaviour_event() {
                Ok(request_response::Event::Message {
                    peer,
                    message:
                        request_response::Message::Request {
                            request, channel, ..
                        },
                }) => {
                    assert_eq!(request, Action::FailOnReadResponse);
                    assert_eq!(&peer, &peer2_id);
                    swarm1
                        .behaviour_mut()
                        .send_response(channel, Action::FailOnReadResponse)
                        .unwrap();
                }
                Ok(request_response::Event::ResponseSent { peer, .. }) => {
                    assert_eq!(&peer, &peer2_id);
                }
                Ok(e) => {
                    panic!("Peer1: Unexpected event: {e:?}")
                }
                Err(..) => {}
            }
        }
    };

    // Expects OutboundFailure::Io failure with `FailOnReadResponse` error
    let swarm2_task = async move {
        let req_id = swarm2
            .behaviour_mut()
            .send_request(&peer1_id, Action::FailOnReadResponse);

        loop {
            match swarm2.select_next_some().await.try_into_behaviour_event() {
                Ok(request_response::Event::OutboundFailure {
                    peer,
                    request_id,
                    error,
                }) => {
                    assert_eq!(&peer, &peer1_id);
                    assert_eq!(request_id, req_id);
                    let error = match error {
                        OutboundFailure::Io(e) => e,
                        e => panic!("Peer2: Unexpected error {e:?}"),
                    };
                    assert_eq!(error.kind(), io::ErrorKind::Other);
                    assert_eq!(
                        error.into_inner().unwrap().to_string(),
                        "FailOnReadResponse"
                    );
                    break;
                }
                Ok(ev) => {
                    panic!("Peer2: Unexpected event: {ev:?}")
                }
                Err(..) => {}
            }
        }
    };

    spawn(swarm1_task);
    timeout(Duration::from_millis(100), swarm2_task)
        .await
        .expect("timed out on waiting FailOnReadResponse");
}

#[async_std::test]
async fn report_outbound_failure_on_write_request() {
    let _ = env_logger::try_init();

    let protocols = iter::once((StreamProtocol::new("/test/1"), ProtocolSupport::Full));
    let cfg = request_response::Config::default();

    let mut swarm1 = Swarm::new_ephemeral(|_| {
        request_response::Behaviour::<TestCodec>::new(protocols.clone(), cfg.clone())
    });
    let peer1_id = *swarm1.local_peer_id();

    let mut swarm2 = Swarm::new_ephemeral(|_| {
        request_response::Behaviour::<TestCodec>::new(protocols.clone(), cfg.clone())
    });

    swarm1.listen().await;
    swarm2.connect(&mut swarm1).await;

    // Consume everything
    let swarm1_task = async move {
        loop {
            swarm1.select_next_some().await;
        }
    };

    // Expects OutboundFailure::Io failure with `FailOnWriteRequest` error
    let swarm2_task = async move {
        let req_id = swarm2
            .behaviour_mut()
            .send_request(&peer1_id, Action::FailOnWriteRequest);

        loop {
            match swarm2.select_next_some().await.try_into_behaviour_event() {
                Ok(request_response::Event::OutboundFailure {
                    peer,
                    request_id,
                    error,
                }) => {
                    assert_eq!(&peer, &peer1_id);
                    assert_eq!(request_id, req_id);
                    let error = match error {
                        OutboundFailure::Io(e) => e,
                        e => panic!("Peer2: Unexpected error {e:?}"),
                    };
                    assert_eq!(error.kind(), io::ErrorKind::Other);
                    assert_eq!(
                        error.into_inner().unwrap().to_string(),
                        "FailOnWriteRequest"
                    );
                    break;
                }
                Ok(ev) => {
                    panic!("Peer2: Unexpected event: {ev:?}")
                }
                Err(..) => {}
            }
        }
    };

    spawn(swarm1_task);
    timeout(Duration::from_millis(100), swarm2_task)
        .await
        .expect("timed out on waiting FailOnReadResponse");
}

#[async_std::test]
async fn report_outbound_timeout_on_read_response() {
    let _ = env_logger::try_init();

    let protocols = iter::once((StreamProtocol::new("/test/1"), ProtocolSupport::Full));
    let cfg = request_response::Config::default();

    let mut swarm1 = Swarm::new_ephemeral(|_| {
        request_response::Behaviour::<TestCodec>::new(protocols.clone(), cfg.clone())
    });
    let peer1_id = *swarm1.local_peer_id();

    let mut swarm2 = Swarm::new_ephemeral(|_| {
        request_response::Behaviour::<TestCodec>::new(
            protocols.clone(),
            cfg.with_request_timeout(Duration::from_millis(100)),
        )
    });
    let peer2_id = *swarm2.local_peer_id();

    swarm1.listen().await;
    swarm2.connect(&mut swarm1).await;

    let (panic_check_tx, panic_check_rx) = channel::bounded::<()>(1);

    // Expects Action::TimeoutOnReadResponse, replies with Action::TimeoutOnReadResponse
    let swarm1_task = async move {
        let _panic_check_tx = panic_check_tx;

        loop {
            match swarm1.select_next_some().await.try_into_behaviour_event() {
                Ok(request_response::Event::Message {
                    peer,
                    message:
                        request_response::Message::Request {
                            request, channel, ..
                        },
                }) => {
                    assert_eq!(request, Action::TimeoutOnReadResponse);
                    assert_eq!(&peer, &peer2_id);
                    swarm1
                        .behaviour_mut()
                        .send_response(channel, Action::TimeoutOnReadResponse)
                        .unwrap();
                }
                Ok(request_response::Event::ResponseSent { peer, .. }) => {
                    assert_eq!(&peer, &peer2_id);
                }
                Ok(e) => {
                    panic!("Peer1: Unexpected event: {e:?}")
                }
                Err(..) => {}
            }
        }
    };

    // Expects OutboundFailure::Timeout
    let swarm2_task = async move {
        let req_id = swarm2
            .behaviour_mut()
            .send_request(&peer1_id, Action::TimeoutOnReadResponse);

        loop {
            match swarm2.select_next_some().await.try_into_behaviour_event() {
                Ok(request_response::Event::OutboundFailure {
                    peer,
                    request_id,
                    error,
                }) => {
                    assert_eq!(&peer, &peer1_id);
                    assert_eq!(request_id, req_id);
                    assert!(matches!(error, OutboundFailure::Timeout));
                    break;
                }
                Ok(ev) => {
                    panic!("Peer2: Unexpected event: {ev:?}")
                }
                Err(..) => {}
            }
        }
    };

    spawn(swarm1_task);
    timeout(Duration::from_millis(200), swarm2_task)
        .await
        .expect("timed out on waiting TimeoutOnReadResponse");

    // Make sure panic wasn't a side effect of by a panic
    assert!(!panic_check_rx.is_closed(), "swarm1_task panicked");
}

#[derive(Clone, Default)]
struct TestCodec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    FailOnReadRequest,
    FailOnReadResponse,
    FailOnWriteRequest,
    FailOnWriteResponse,
    TimeoutOnReadRequest,
    TimeoutOnReadResponse,
}

impl From<Action> for u8 {
    fn from(value: Action) -> Self {
        match value {
            Action::FailOnReadRequest => 0,
            Action::FailOnReadResponse => 1,
            Action::FailOnWriteRequest => 2,
            Action::FailOnWriteResponse => 3,
            Action::TimeoutOnReadRequest => 4,
            Action::TimeoutOnReadResponse => 5,
        }
    }
}

impl TryFrom<u8> for Action {
    type Error = io::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Action::FailOnReadRequest),
            1 => Ok(Action::FailOnReadResponse),
            2 => Ok(Action::FailOnWriteRequest),
            3 => Ok(Action::FailOnWriteResponse),
            4 => Ok(Action::TimeoutOnReadRequest),
            5 => Ok(Action::TimeoutOnReadResponse),
            _ => Err(io::Error::new(io::ErrorKind::Other, "invalid action")),
        }
    }
}

#[async_trait]
impl Codec for TestCodec {
    type Protocol = StreamProtocol;
    type Request = Action;
    type Response = Action;

    async fn read_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut buf = Vec::new();
        io.read_to_end(&mut buf).await?;
        assert_eq!(buf.len(), 1);

        match buf[0].try_into()? {
            Action::FailOnReadRequest => {
                Err(io::Error::new(io::ErrorKind::Other, "FailOnReadRequest"))
            }
            Action::TimeoutOnReadRequest => loop {
                sleep(Duration::MAX).await;
            },
            action => Ok(action),
        }
    }

    async fn read_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut buf = Vec::new();
        io.read_to_end(&mut buf).await?;
        assert_eq!(buf.len(), 1);

        match buf[0].try_into()? {
            Action::FailOnReadResponse => {
                Err(io::Error::new(io::ErrorKind::Other, "FailOnReadResponse"))
            }
            Action::TimeoutOnReadResponse => loop {
                sleep(Duration::MAX).await;
            },
            action => Ok(action),
        }
    }

    async fn write_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        // Even on FailOnWriteRequest we write to the stream, this is
        // because we don't want the other read_request to fail on the
        // other end.
        let bytes = [req.into()];
        io.write_all(&bytes).await?;
        io.flush().await?;

        if req == Action::FailOnWriteRequest {
            Err(io::Error::new(io::ErrorKind::Other, "FailOnWriteRequest"))
        } else {
            Ok(())
        }
    }

    async fn write_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
        res: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        match res {
            Action::FailOnWriteResponse => {
                Err(io::Error::new(io::ErrorKind::Other, "FailOnWriteResponse"))
            }
            action => {
                let bytes = [action.into()];
                io.write_all(&bytes).await?;
                Ok(())
            }
        }
    }
}
