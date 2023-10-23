use async_std::channel;
use async_std::future::timeout;
use async_std::task::{sleep, spawn};
use async_trait::async_trait;
use futures::prelude::*;
use libp2p_request_response as request_response;
use libp2p_request_response::ProtocolSupport;
use libp2p_swarm::{StreamProtocol, Swarm};
use libp2p_swarm_test::SwarmExt;
use request_response::{Codec, InboundFailure, OutboundFailure};
use std::time::Duration;
use std::{io, iter};

#[async_std::test]
async fn report_outbound_failure_on_read_response() {
    let _ = env_logger::try_init();

    let protocols = iter::once((StreamProtocol::new("/test/1"), ProtocolSupport::Full));
    let cfg = request_response::Config::default();
    let codec = TestCodec(Action::FailOnReadResponse);

    let mut swarm1 = Swarm::new_ephemeral(|_| {
        request_response::Behaviour::with_codec(codec.clone(), protocols.clone(), cfg.clone())
    });
    let peer1_id = *swarm1.local_peer_id();

    let mut swarm2 =
        Swarm::new_ephemeral(|_| request_response::Behaviour::with_codec(codec, protocols, cfg));
    let peer2_id = *swarm2.local_peer_id();

    swarm1.listen().await;
    swarm2.connect(&mut swarm1).await;

    let swarm1_task = async move {
        loop {
            match swarm1.select_next_some().await.try_into_behaviour_event() {
                Ok(request_response::Event::Message {
                    peer,
                    message: request_response::Message::Request { channel, .. },
                }) => {
                    assert_eq!(&peer, &peer2_id);
                    swarm1.behaviour_mut().send_response(channel, ()).unwrap();
                }
                Ok(request_response::Event::ResponseSent { peer, .. }) => {
                    assert_eq!(&peer, &peer2_id);
                    break;
                }
                Ok(ev) => {
                    panic!("Peer1: Unexpected event: {ev:?}")
                }
                Err(..) => {}
            }
        }
    };

    // Expects OutboundFailure::Io failure with `FailOnReadResponse` error
    let swarm2_task = async move {
        let req_id = swarm2.behaviour_mut().send_request(&peer1_id, ());

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

    let join_handle = spawn(swarm1_task);

    timeout(Duration::from_millis(100), swarm2_task)
        .await
        .expect("timed out on waiting FailOnReadResponse");

    join_handle.await;
}

#[async_std::test]
async fn report_outbound_failure_on_write_request() {
    let _ = env_logger::try_init();

    let protocols = iter::once((StreamProtocol::new("/test/1"), ProtocolSupport::Full));
    let cfg = request_response::Config::default();
    let codec = TestCodec(Action::FailOnWriteRequest);

    let mut swarm1 = Swarm::new_ephemeral(|_| {
        request_response::Behaviour::with_codec(codec.clone(), protocols.clone(), cfg.clone())
    });
    let peer1_id = *swarm1.local_peer_id();

    let mut swarm2 =
        Swarm::new_ephemeral(|_| request_response::Behaviour::with_codec(codec, protocols, cfg));

    swarm1.listen().await;
    swarm2.connect(&mut swarm1).await;

    let swarm1_task = async move {
        // No need to take any actions, just consume everything.
        loop {
            swarm1.select_next_some().await;
        }
    };

    // Expects OutboundFailure::Io failure with `FailOnWriteRequest` error.
    let swarm2_task = async move {
        let req_id = swarm2.behaviour_mut().send_request(&peer1_id, ());

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
    let codec = TestCodec(Action::TimeoutOnReadResponse);

    let mut swarm1 = Swarm::new_ephemeral(|_| {
        request_response::Behaviour::with_codec(codec.clone(), protocols.clone(), cfg.clone())
    });
    let peer1_id = *swarm1.local_peer_id();

    let mut swarm2 = Swarm::new_ephemeral(|_| {
        let cfg = cfg.with_request_timeout(Duration::from_millis(100));
        request_response::Behaviour::with_codec(codec, protocols, cfg)
    });
    let peer2_id = *swarm2.local_peer_id();

    swarm1.listen().await;
    swarm2.connect(&mut swarm1).await;

    let (panic_check_tx, panic_check_rx) = channel::bounded::<()>(1);

    let swarm1_task = async move {
        // Connection needs to be kept alive, so the folloing loop should not break.
        // This channel is used to check if `swarm1_task` panicked or not.
        let _panic_check_tx = panic_check_tx;

        loop {
            match swarm1.select_next_some().await.try_into_behaviour_event() {
                Ok(request_response::Event::Message {
                    peer,
                    message: request_response::Message::Request { channel, .. },
                }) => {
                    assert_eq!(&peer, &peer2_id);
                    swarm1.behaviour_mut().send_response(channel, ()).unwrap();
                }
                Ok(request_response::Event::ResponseSent { peer, .. }) => {
                    assert_eq!(&peer, &peer2_id);
                }
                Ok(ev) => {
                    panic!("Peer1: Unexpected event: {ev:?}")
                }
                Err(..) => {}
            }
        }
    };

    // Expects OutboundFailure::Timeout
    let swarm2_task = async move {
        let req_id = swarm2.behaviour_mut().send_request(&peer1_id, ());

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

#[async_std::test]
async fn report_inbound_failure_on_read_request() {
    let _ = env_logger::try_init();

    let protocols = iter::once((StreamProtocol::new("/test/1"), ProtocolSupport::Full));
    let cfg = request_response::Config::default();
    let codec = TestCodec(Action::FailOnReadRequest);

    let mut swarm1 = Swarm::new_ephemeral(|_| {
        request_response::Behaviour::with_codec(codec.clone(), protocols.clone(), cfg.clone())
    });
    let peer1_id = *swarm1.local_peer_id();

    let mut swarm2 =
        Swarm::new_ephemeral(|_| request_response::Behaviour::with_codec(codec, protocols, cfg));
    let peer2_id = *swarm2.local_peer_id();

    swarm1.listen().await;
    swarm2.connect(&mut swarm1).await;

    // Expects OutboundFailure::Io failure with `FailOnReadRequest` error
    let swarm1_task = async move {
        loop {
            match swarm1.select_next_some().await.try_into_behaviour_event() {
                Ok(request_response::Event::InboundFailure { peer, error, .. }) => {
                    assert_eq!(peer, peer2_id);

                    let error = match error {
                        InboundFailure::Io(e) => e,
                        e => panic!("Peer1: Unexpected error {e:?}"),
                    };

                    assert_eq!(error.kind(), io::ErrorKind::Other);
                    assert_eq!(error.into_inner().unwrap().to_string(), "FailOnReadRequest");
                    break;
                }
                Ok(ev) => panic!("Peer1: Unexpected event: {ev:?}"),
                Err(..) => {}
            }
        }
    };

    let swarm2_task = async move {
        let _req_id = swarm2.behaviour_mut().send_request(&peer1_id, ());

        // No need to take any actions, just consume everything.
        loop {
            swarm2.select_next_some().await;
        }
    };

    spawn(swarm2_task);
    timeout(Duration::from_millis(100), swarm1_task)
        .await
        .expect("timed out on waiting FailOnWriteRequest");
}

#[derive(Clone)]
struct TestCodec(Action);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    FailOnReadRequest,
    FailOnReadResponse,
    FailOnWriteRequest,
    FailOnWriteResponse,
    TimeoutOnReadRequest,
    TimeoutOnReadResponse,
}

#[async_trait]
impl Codec for TestCodec {
    type Protocol = StreamProtocol;
    type Request = ();
    type Response = ();

    async fn read_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        _io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        match self.0 {
            Action::FailOnReadRequest => {
                Err(io::Error::new(io::ErrorKind::Other, "FailOnReadRequest"))
            }
            Action::TimeoutOnReadRequest => loop {
                sleep(Duration::MAX).await;
            },
            _ => Ok(()),
        }
    }

    async fn read_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        _io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        match self.0 {
            Action::FailOnReadResponse => {
                Err(io::Error::new(io::ErrorKind::Other, "FailOnReadResponse"))
            }
            Action::TimeoutOnReadResponse => loop {
                sleep(Duration::MAX).await;
            },
            _ => Ok(()),
        }
    }

    async fn write_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        _io: &mut T,
        _req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        match self.0 {
            Action::FailOnWriteRequest => {
                Err(io::Error::new(io::ErrorKind::Other, "FailOnWriteRequest"))
            }
            _ => Ok(()),
        }
    }

    async fn write_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        _io: &mut T,
        _res: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        match self.0 {
            Action::FailOnWriteResponse => {
                Err(io::Error::new(io::ErrorKind::Other, "FailOnWriteResponse"))
            }
            _ => Ok(()),
        }
    }
}
