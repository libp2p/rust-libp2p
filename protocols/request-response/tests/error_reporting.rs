use anyhow::{bail, Result};
use async_std::channel;
use async_std::future::timeout;
use async_std::task::{sleep, spawn};
use async_trait::async_trait;
use futures::prelude::*;
use libp2p_identity::PeerId;
use libp2p_request_response as request_response;
use libp2p_request_response::ProtocolSupport;
use libp2p_swarm::{StreamProtocol, Swarm};
use libp2p_swarm_test::SwarmExt;
use request_response::{
    Codec, InboundFailure, InboundRequestId, OutboundFailure, OutboundRequestId, ResponseChannel,
};
use std::time::Duration;
use std::{io, iter};

#[async_std::test]
async fn report_outbound_failure_on_read_response() {
    let _ = env_logger::try_init();

    let (peer1_id, mut swarm1) = new_swarm_with_timeout(Duration::from_millis(100));
    let (peer2_id, mut swarm2) = new_swarm_with_timeout(Duration::from_millis(100));

    swarm1.listen().await;
    swarm2.connect(&mut swarm1).await;

    let swarm1_task = async move {
        let (peer, req_id, action, resp_channel) = wait_request(&mut swarm1).await.unwrap();
        assert_eq!(peer, peer2_id);
        assert_eq!(action, Action::FailOnReadResponse);
        swarm1
            .behaviour_mut()
            .send_response(resp_channel, Action::FailOnReadResponse)
            .unwrap();

        let (peer, req_id_done) = wait_response_sent(&mut swarm1).await.unwrap();
        assert_eq!(peer, peer2_id);
        assert_eq!(req_id_done, req_id);

        // Wait a bit for the other side
        sleep(Duration::from_millis(10)).await;
    };

    let swarm2_task = async move {
        let req_id = swarm2
            .behaviour_mut()
            .send_request(&peer1_id, Action::FailOnReadResponse);

        let (peer, req_id_done, error) = wait_outbound_failure(&mut swarm2).await.unwrap();
        assert_eq!(peer, peer1_id);
        assert_eq!(req_id_done, req_id);

        let error = match error {
            OutboundFailure::Io(e) => e,
            e => panic!("Unexpected error {e:?}"),
        };

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(
            error.into_inner().unwrap().to_string(),
            "FailOnReadResponse"
        );
    };

    futures::future::join(swarm1_task, swarm2_task).await;
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

    let mut swarm2 =
        Swarm::new_ephemeral(|_| request_response::Behaviour::<TestCodec>::new(protocols, cfg));

    swarm1.listen().await;
    swarm2.connect(&mut swarm1).await;

    // On panic `panic_check_rx` will be closed
    let (panic_check_tx, panic_check_rx) = channel::bounded::<()>(1);

    // Server
    //
    // Expects no events because `Event::Request` is produced after `read_request`.
    let swarm1_task = async move {
        let _panic_check_tx = panic_check_tx;

        loop {
            match swarm1.select_next_some().await.try_into_behaviour_event() {
                Ok(ev) => panic!("Peer1: Unexpected event: {ev:?}"),
                Err(..) => {}
            }
        }
    };

    // Client
    //
    // Expects OutboundFailure::Io failure with `FailOnWriteRequest` error.
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
                    assert_eq!(peer, peer1_id);
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
                Ok(ev) => panic!("Peer2: Unexpected event: {ev:?}"),
                Err(..) => {}
            }
        }
    };

    spawn(swarm1_task);
    timeout(Duration::from_millis(100), swarm2_task)
        .await
        .expect("timed out on waiting FailOnWriteRequest");

    assert!(!panic_check_rx.is_closed(), "swarm1_task panicked");
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
        let cfg = cfg.with_request_timeout(Duration::from_millis(100));
        request_response::Behaviour::<TestCodec>::new(protocols, cfg)
    });
    let peer2_id = *swarm2.local_peer_id();

    swarm1.listen().await;
    swarm2.connect(&mut swarm1).await;

    // On panic `panic_check_rx` will be closed
    let (panic_check_tx, panic_check_rx) = channel::bounded::<()>(1);

    // Server
    let swarm1_task = async move {
        let _panic_check_tx = panic_check_tx;
        let mut req_id = None;

        loop {
            match swarm1.select_next_some().await.try_into_behaviour_event() {
                Ok(request_response::Event::Message {
                    peer,
                    message:
                        request_response::Message::Request {
                            request_id,
                            request,
                            channel,
                        },
                }) => {
                    assert_eq!(peer, peer2_id);
                    assert_eq!(request, Action::TimeoutOnReadResponse);
                    req_id = Some(request_id);
                    swarm1
                        .behaviour_mut()
                        .send_response(channel, Action::TimeoutOnReadResponse)
                        .unwrap();
                }
                Ok(request_response::Event::ResponseSent {
                    peer, request_id, ..
                }) => {
                    assert_eq!(peer, peer2_id);
                    assert_eq!(req_id, Some(request_id));
                }
                Ok(ev) => panic!("Peer1: Unexpected event: {ev:?}"),
                Err(..) => {}
            }
        }
    };

    // Client
    //
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
                    assert_eq!(peer, peer1_id);
                    assert_eq!(request_id, req_id);
                    assert!(matches!(error, OutboundFailure::Timeout));
                    break;
                }
                Ok(ev) => panic!("Peer2: Unexpected event: {ev:?}"),
                Err(..) => {}
            }
        }
    };

    spawn(swarm1_task);
    timeout(Duration::from_millis(200), swarm2_task)
        .await
        .expect("timed out on waiting TimeoutOnReadResponse");

    assert!(!panic_check_rx.is_closed(), "swarm1_task panicked");
}

#[async_std::test]
async fn report_inbound_failure_on_read_request() {
    let _ = env_logger::try_init();

    let protocols = iter::once((StreamProtocol::new("/test/1"), ProtocolSupport::Full));
    let cfg = request_response::Config::default();

    let mut swarm1 = Swarm::new_ephemeral(|_| {
        request_response::Behaviour::<TestCodec>::new(protocols.clone(), cfg.clone())
    });
    let peer1_id = *swarm1.local_peer_id();

    let mut swarm2 =
        Swarm::new_ephemeral(|_| request_response::Behaviour::<TestCodec>::new(protocols, cfg));

    swarm1.listen().await;
    swarm2.connect(&mut swarm1).await;

    // On panic `panic_check_rx` will be closed
    let (panic_check_tx, panic_check_rx) = channel::bounded::<()>(1);

    // Server
    //
    // Expects no events because `Event::Request` is produced after `read_request`.
    let swarm1_task = async move {
        let _panic_check_tx = panic_check_tx;

        loop {
            match swarm1.select_next_some().await.try_into_behaviour_event() {
                Ok(ev) => panic!("Peer1: Unexpected event: {ev:?}"),
                Err(..) => {}
            }
        }
    };

    // Expects io::ErrorKind::UnexpectedEof
    let swarm2_task = async move {
        let req_id = swarm2
            .behaviour_mut()
            .send_request(&peer1_id, Action::FailOnReadRequest);

        loop {
            match swarm2.select_next_some().await.try_into_behaviour_event() {
                Ok(request_response::Event::OutboundFailure {
                    peer,
                    request_id,
                    error,
                }) => {
                    assert_eq!(peer, peer1_id);
                    assert_eq!(request_id, req_id);

                    let error = match error {
                        OutboundFailure::Io(e) => e,
                        e => panic!("Peer2: Unexpected error {e:?}"),
                    };

                    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
                    break;
                }
                Ok(ev) => panic!("Peer2: Unexpected event: {ev:?}"),
                Err(..) => {}
            }
        }
    };

    spawn(swarm1_task);
    timeout(Duration::from_millis(100), swarm2_task)
        .await
        .expect("timed out on waiting FailOnWriteRequest");

    assert!(!panic_check_rx.is_closed(), "swarm1_task panicked");
}

#[async_std::test]
async fn report_inbound_failure_on_write_response() {
    let _ = env_logger::try_init();

    let protocols = iter::once((StreamProtocol::new("/test/1"), ProtocolSupport::Full));
    let cfg = request_response::Config::default();

    let mut swarm1 = Swarm::new_ephemeral(|_| {
        request_response::Behaviour::<TestCodec>::new(protocols.clone(), cfg.clone())
    });
    let peer1_id = *swarm1.local_peer_id();

    let mut swarm2 =
        Swarm::new_ephemeral(|_| request_response::Behaviour::<TestCodec>::new(protocols, cfg));
    let peer2_id = *swarm2.local_peer_id();

    swarm1.listen().await;
    swarm2.connect(&mut swarm1).await;

    // On panic `panic_check_rx` will be closed
    let (panic_check_tx, panic_check_rx) = channel::bounded::<()>(1);

    // Server
    //
    // Expects OutboundFailure::Io failure with `FailOnWriteResponse` error
    let swarm1_task = async move {
        let mut req_id = None;

        loop {
            match swarm1.select_next_some().await.try_into_behaviour_event() {
                Ok(request_response::Event::Message {
                    peer,
                    message:
                        request_response::Message::Request {
                            request_id,
                            request,
                            channel,
                        },
                }) => {
                    assert_eq!(peer, peer2_id);
                    assert_eq!(request, Action::FailOnWriteResponse);
                    req_id = Some(request_id);
                    swarm1
                        .behaviour_mut()
                        .send_response(channel, Action::FailOnWriteResponse)
                        .unwrap();
                }
                Ok(request_response::Event::InboundFailure {
                    peer,
                    request_id,
                    error,
                }) => {
                    assert_eq!(peer, peer2_id);
                    assert_eq!(req_id, Some(request_id));

                    let error = match error {
                        InboundFailure::Io(e) => e,
                        e => panic!("Peer1: Unexpected error {e:?}"),
                    };

                    assert_eq!(error.kind(), io::ErrorKind::Other);
                    assert_eq!(
                        error.into_inner().unwrap().to_string(),
                        "FailOnWriteResponse"
                    );
                    break;
                }
                Ok(ev) => panic!("Peer1: Unexpected event: {ev:?}"),
                Err(..) => {}
            }
        }
    };

    // Client
    //
    // Expects OutboundFailure::ConnectionClosed or io::ErrorKind::UnexpectedEof
    let swarm2_task = async move {
        let _panic_check_tx = panic_check_tx;
        let req_id = swarm2
            .behaviour_mut()
            .send_request(&peer1_id, Action::FailOnWriteResponse);

        loop {
            match swarm2.select_next_some().await.try_into_behaviour_event() {
                Ok(request_response::Event::OutboundFailure {
                    peer,
                    request_id,
                    error,
                }) => {
                    assert_eq!(peer, peer1_id);
                    assert_eq!(request_id, req_id);

                    match error {
                        OutboundFailure::ConnectionClosed => {
                            // Connections was closed before `read_response`
                        }
                        OutboundFailure::Io(e) => {
                            assert_eq!(e.kind(), io::ErrorKind::UnexpectedEof);
                        }
                        e => panic!("Peer2: Unexpected error {e:?}"),
                    }
                }
                Ok(ev) => panic!("Peer2: Unexpected event: {ev:?}"),
                Err(..) => {}
            }
        }
    };

    spawn(swarm2_task);
    timeout(Duration::from_millis(100), swarm1_task)
        .await
        .expect("timed out on waiting TimeoutOnWriteResponse");

    assert!(!panic_check_rx.is_closed(), "swarm2_task panicked");
}

#[async_std::test]
async fn report_inbound_timeout_on_write_response() {
    let _ = env_logger::try_init();

    let protocols = iter::once((StreamProtocol::new("/test/1"), ProtocolSupport::Full));
    let cfg = request_response::Config::default();

    let mut swarm1 = Swarm::new_ephemeral(|_| {
        let cfg = cfg.clone().with_request_timeout(Duration::from_millis(100));
        request_response::Behaviour::<TestCodec>::new(protocols.clone(), cfg.clone())
    });
    let peer1_id = *swarm1.local_peer_id();

    let mut swarm2 =
        Swarm::new_ephemeral(|_| request_response::Behaviour::<TestCodec>::new(protocols, cfg));
    let peer2_id = *swarm2.local_peer_id();

    swarm1.listen().await;
    swarm2.connect(&mut swarm1).await;

    // On panic `panic_check_rx` will be closed
    let (panic_check_tx, panic_check_rx) = channel::bounded::<()>(1);

    // Expects InboundFailure::Timeout
    let swarm1_task = async move {
        let mut req_id = None;

        loop {
            match swarm1.select_next_some().await.try_into_behaviour_event() {
                Ok(request_response::Event::Message {
                    peer,
                    message:
                        request_response::Message::Request {
                            request_id,
                            request,
                            channel,
                        },
                }) => {
                    assert_eq!(peer, peer2_id);
                    assert_eq!(request, Action::TimeoutOnWriteResponse);
                    req_id = Some(request_id);
                    swarm1
                        .behaviour_mut()
                        .send_response(channel, Action::TimeoutOnWriteResponse)
                        .unwrap();
                }
                Ok(request_response::Event::InboundFailure {
                    peer,
                    request_id,
                    error,
                }) => {
                    assert_eq!(peer, peer2_id);
                    assert_eq!(req_id, Some(request_id));
                    assert!(matches!(error, InboundFailure::Timeout));
                    break;
                }
                Ok(ev) => panic!("Peer1: Unexpected event: {ev:?}"),
                Err(..) => {}
            }
        }
    };

    // Expects OutboundFailure::ConnectionClosed or io::ErrorKind::UnexpectedEof
    let swarm2_task = async move {
        let _panic_check_tx = panic_check_tx;
        let req_id = swarm2
            .behaviour_mut()
            .send_request(&peer1_id, Action::TimeoutOnWriteResponse);

        loop {
            match swarm2.select_next_some().await.try_into_behaviour_event() {
                Ok(request_response::Event::OutboundFailure {
                    peer,
                    request_id,
                    error,
                }) => {
                    assert_eq!(peer, peer1_id);
                    assert_eq!(request_id, req_id);

                    match error {
                        OutboundFailure::ConnectionClosed => {
                            // Connections was closed before `read_response`
                        }
                        OutboundFailure::Io(e) => {
                            assert_eq!(e.kind(), io::ErrorKind::UnexpectedEof)
                        }
                        e => panic!("Peer2: Unexpected error {e:?}"),
                    }
                }
                Ok(ev) => panic!("Peer2: Unexpected event: {ev:?}"),
                Err(..) => {}
            }
        }
    };

    spawn(swarm2_task);
    timeout(Duration::from_millis(200), swarm1_task)
        .await
        .expect("timed out on waiting TimeoutOnWriteResponse");

    assert!(!panic_check_rx.is_closed(), "swarm2_task panicked");
}

fn new_swarm_with_timeout(
    timeout: Duration,
) -> (PeerId, Swarm<request_response::Behaviour<TestCodec>>) {
    let protocols = iter::once((StreamProtocol::new("/test/1"), ProtocolSupport::Full));
    let cfg = request_response::Config::default().with_request_timeout(timeout);

    let swarm =
        Swarm::new_ephemeral(|_| request_response::Behaviour::<TestCodec>::new(protocols, cfg));
    let peed_id = *swarm.local_peer_id();

    (peed_id, swarm)
}

async fn wait_request(
    swarm: &mut Swarm<request_response::Behaviour<TestCodec>>,
) -> Result<(PeerId, InboundRequestId, Action, ResponseChannel<Action>)> {
    loop {
        match swarm.select_next_some().await.try_into_behaviour_event() {
            Ok(request_response::Event::Message {
                peer,
                message:
                    request_response::Message::Request {
                        request_id,
                        request,
                        channel,
                    },
            }) => {
                return Ok((peer, request_id, request, channel));
            }
            Ok(ev) => bail!("Unexpected event: {ev:?}"),
            Err(..) => {}
        }
    }
}

async fn wait_response_sent(
    swarm: &mut Swarm<request_response::Behaviour<TestCodec>>,
) -> Result<(PeerId, InboundRequestId)> {
    loop {
        match swarm.select_next_some().await.try_into_behaviour_event() {
            Ok(request_response::Event::ResponseSent {
                peer, request_id, ..
            }) => {
                return Ok((peer, request_id));
            }
            Ok(ev) => bail!("Unexpected event: {ev:?}"),
            Err(..) => {}
        }
    }
}

async fn wait_outbound_failure(
    swarm: &mut Swarm<request_response::Behaviour<TestCodec>>,
) -> Result<(PeerId, OutboundRequestId, OutboundFailure)> {
    loop {
        match swarm.select_next_some().await.try_into_behaviour_event() {
            Ok(request_response::Event::OutboundFailure {
                peer,
                request_id,
                error,
            }) => {
                return Ok((peer, request_id, error));
            }
            Ok(ev) => bail!("Unexpected event: {ev:?}"),
            Err(..) => {}
        }
    }
}

#[derive(Clone, Default)]
struct TestCodec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    FailOnReadRequest,
    FailOnReadResponse,
    TimeoutOnReadResponse,
    FailOnWriteRequest,
    FailOnWriteResponse,
    TimeoutOnWriteResponse,
}

impl From<Action> for u8 {
    fn from(value: Action) -> Self {
        match value {
            Action::FailOnReadRequest => 0,
            Action::FailOnReadResponse => 1,
            Action::TimeoutOnReadResponse => 2,
            Action::FailOnWriteRequest => 3,
            Action::FailOnWriteResponse => 4,
            Action::TimeoutOnWriteResponse => 5,
        }
    }
}

impl TryFrom<u8> for Action {
    type Error = io::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Action::FailOnReadRequest),
            1 => Ok(Action::FailOnReadResponse),
            2 => Ok(Action::TimeoutOnReadResponse),
            3 => Ok(Action::FailOnWriteRequest),
            4 => Ok(Action::FailOnWriteResponse),
            5 => Ok(Action::TimeoutOnWriteResponse),
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

        if buf.is_empty() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }

        assert_eq!(buf.len(), 1);

        match buf[0].try_into()? {
            Action::FailOnReadRequest => {
                Err(io::Error::new(io::ErrorKind::Other, "FailOnReadRequest"))
            }
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

        if buf.is_empty() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }

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
        match req {
            Action::FailOnWriteRequest => {
                Err(io::Error::new(io::ErrorKind::Other, "FailOnWriteRequest"))
            }
            action => {
                let bytes = [action.into()];
                io.write_all(&bytes).await?;
                Ok(())
            }
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
            Action::TimeoutOnWriteResponse => loop {
                sleep(Duration::MAX).await;
            },
            action => {
                let bytes = [action.into()];
                io.write_all(&bytes).await?;
                Ok(())
            }
        }
    }
}
