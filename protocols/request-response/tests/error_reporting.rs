use std::{io, iter, pin::pin, time::Duration};

use anyhow::{bail, Result};
use async_std::task::sleep;
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
use tracing_subscriber::EnvFilter;

#[async_std::test]
async fn report_outbound_failure_on_read_response() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let (peer1_id, mut swarm1) = new_swarm();
    let (peer2_id, mut swarm2) = new_swarm();

    swarm1.listen().with_memory_addr_external().await;
    swarm2.connect(&mut swarm1).await;

    let server_task = async move {
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

        // Keep the connection alive, otherwise swarm2 may receive `ConnectionClosed` instead
        wait_no_events(&mut swarm1).await;
    };

    // Expects OutboundFailure::Io failure with `FailOnReadResponse` error
    let client_task = async move {
        let req_id = swarm2
            .behaviour_mut()
            .send_request(&peer1_id, Action::FailOnReadResponse);

        let (peer, req_id_done, error) = wait_outbound_failure(&mut swarm2).await.unwrap();
        assert_eq!(peer, peer1_id);
        assert_eq!(req_id_done, req_id);

        let error = match error {
            OutboundFailure::Io(e) => e,
            e => panic!("Unexpected error: {e:?}"),
        };

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(
            error.into_inner().unwrap().to_string(),
            "FailOnReadResponse"
        );
    };

    let server_task = pin!(server_task);
    let client_task = pin!(client_task);
    futures::future::select(server_task, client_task).await;
}

#[async_std::test]
async fn report_outbound_failure_on_write_request() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let (peer1_id, mut swarm1) = new_swarm();
    let (_peer2_id, mut swarm2) = new_swarm();

    swarm1.listen().with_memory_addr_external().await;
    swarm2.connect(&mut swarm1).await;

    // Expects no events because `Event::Request` is produced after `read_request`.
    // Keep the connection alive, otherwise swarm2 may receive `ConnectionClosed` instead.
    let server_task = wait_no_events(&mut swarm1);

    // Expects OutboundFailure::Io failure with `FailOnWriteRequest` error.
    let client_task = async move {
        let req_id = swarm2
            .behaviour_mut()
            .send_request(&peer1_id, Action::FailOnWriteRequest);

        let (peer, req_id_done, error) = wait_outbound_failure(&mut swarm2).await.unwrap();
        assert_eq!(peer, peer1_id);
        assert_eq!(req_id_done, req_id);

        let error = match error {
            OutboundFailure::Io(e) => e,
            e => panic!("Unexpected error: {e:?}"),
        };

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(
            error.into_inner().unwrap().to_string(),
            "FailOnWriteRequest"
        );
    };

    let server_task = pin!(server_task);
    let client_task = pin!(client_task);
    futures::future::select(server_task, client_task).await;
}

#[async_std::test]
async fn report_outbound_timeout_on_read_response() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    // `swarm1` needs to have a bigger timeout to avoid racing
    let (peer1_id, mut swarm1) = new_swarm_with_timeout(Duration::from_millis(200));
    let (peer2_id, mut swarm2) = new_swarm_with_timeout(Duration::from_millis(100));

    swarm1.listen().with_memory_addr_external().await;
    swarm2.connect(&mut swarm1).await;

    let server_task = async move {
        let (peer, req_id, action, resp_channel) = wait_request(&mut swarm1).await.unwrap();
        assert_eq!(peer, peer2_id);
        assert_eq!(action, Action::TimeoutOnReadResponse);
        swarm1
            .behaviour_mut()
            .send_response(resp_channel, Action::TimeoutOnReadResponse)
            .unwrap();

        let (peer, req_id_done) = wait_response_sent(&mut swarm1).await.unwrap();
        assert_eq!(peer, peer2_id);
        assert_eq!(req_id_done, req_id);

        // Keep the connection alive, otherwise swarm2 may receive `ConnectionClosed` instead
        wait_no_events(&mut swarm1).await;
    };

    // Expects OutboundFailure::Timeout
    let client_task = async move {
        let req_id = swarm2
            .behaviour_mut()
            .send_request(&peer1_id, Action::TimeoutOnReadResponse);

        let (peer, req_id_done, error) = wait_outbound_failure(&mut swarm2).await.unwrap();
        assert_eq!(peer, peer1_id);
        assert_eq!(req_id_done, req_id);
        assert!(matches!(error, OutboundFailure::Timeout));
    };

    let server_task = pin!(server_task);
    let client_task = pin!(client_task);
    futures::future::select(server_task, client_task).await;
}

#[async_std::test]
async fn report_outbound_failure_on_max_streams() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    // `swarm2` will be able to handle only 1 stream per time.
    let swarm2_config = request_response::Config::default()
        .with_request_timeout(Duration::from_millis(100))
        .with_max_concurrent_streams(1);

    let (peer1_id, mut swarm1) = new_swarm();
    let (peer2_id, mut swarm2) = new_swarm_with_config(swarm2_config);

    swarm1.listen().with_memory_addr_external().await;
    swarm2.connect(&mut swarm1).await;

    let swarm1_task = async move {
        let _req_id = swarm1
            .behaviour_mut()
            .send_request(&peer2_id, Action::FailOnMaxStreams);

        // Keep the connection alive, otherwise swarm2 may receive `ConnectionClosed` instead.
        wait_no_events(&mut swarm1).await;
    };

    // Expects OutboundFailure::Io failure.
    let swarm2_task = async move {
        let (peer, _inbound_req_id, action, _resp_channel) =
            wait_request(&mut swarm2).await.unwrap();
        assert_eq!(peer, peer1_id);
        assert_eq!(action, Action::FailOnMaxStreams);

        // A task for sending back a response is already scheduled so max concurrent
        // streams is reached and no new tasks can be scheduled.
        //
        // We produce the failure by creating new request before we response.
        let outbound_req_id = swarm2
            .behaviour_mut()
            .send_request(&peer1_id, Action::FailOnMaxStreams);

        let (peer, req_id_done, error) = wait_outbound_failure(&mut swarm2).await.unwrap();
        assert_eq!(peer, peer1_id);
        assert_eq!(req_id_done, outbound_req_id);
        assert!(matches!(error, OutboundFailure::Io(_)));
    };

    let swarm1_task = pin!(swarm1_task);
    let swarm2_task = pin!(swarm2_task);
    futures::future::select(swarm1_task, swarm2_task).await;
}

#[async_std::test]
async fn report_inbound_failure_on_read_request() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let (peer1_id, mut swarm1) = new_swarm();
    let (_peer2_id, mut swarm2) = new_swarm();

    swarm1.listen().with_memory_addr_external().await;
    swarm2.connect(&mut swarm1).await;

    // Expects no events because `Event::Request` is produced after `read_request`.
    // Keep the connection alive, otherwise swarm2 may receive `ConnectionClosed` instead.
    let server_task = wait_no_events(&mut swarm1);

    // Expects io::ErrorKind::UnexpectedEof
    let client_task = async move {
        let req_id = swarm2
            .behaviour_mut()
            .send_request(&peer1_id, Action::FailOnReadRequest);

        let (peer, req_id_done, error) = wait_outbound_failure(&mut swarm2).await.unwrap();
        assert_eq!(peer, peer1_id);
        assert_eq!(req_id_done, req_id);

        match error {
            OutboundFailure::Io(e) if e.kind() == io::ErrorKind::UnexpectedEof => {}
            e => panic!("Unexpected error: {e:?}"),
        };
    };

    let server_task = pin!(server_task);
    let client_task = pin!(client_task);
    futures::future::select(server_task, client_task).await;
}

#[async_std::test]
async fn report_inbound_failure_on_write_response() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let (peer1_id, mut swarm1) = new_swarm();
    let (peer2_id, mut swarm2) = new_swarm();

    swarm1.listen().with_memory_addr_external().await;
    swarm2.connect(&mut swarm1).await;

    // Expects OutboundFailure::Io failure with `FailOnWriteResponse` error
    let server_task = async move {
        let (peer, req_id, action, resp_channel) = wait_request(&mut swarm1).await.unwrap();
        assert_eq!(peer, peer2_id);
        assert_eq!(action, Action::FailOnWriteResponse);
        swarm1
            .behaviour_mut()
            .send_response(resp_channel, Action::FailOnWriteResponse)
            .unwrap();

        let (peer, req_id_done, error) = wait_inbound_failure(&mut swarm1).await.unwrap();
        assert_eq!(peer, peer2_id);
        assert_eq!(req_id_done, req_id);

        let error = match error {
            InboundFailure::Io(e) => e,
            e => panic!("Unexpected error: {e:?}"),
        };

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(
            error.into_inner().unwrap().to_string(),
            "FailOnWriteResponse"
        );
    };

    // Expects OutboundFailure::ConnectionClosed or io::ErrorKind::UnexpectedEof
    let client_task = async move {
        let req_id = swarm2
            .behaviour_mut()
            .send_request(&peer1_id, Action::FailOnWriteResponse);

        let (peer, req_id_done, error) = wait_outbound_failure(&mut swarm2).await.unwrap();
        assert_eq!(peer, peer1_id);
        assert_eq!(req_id_done, req_id);

        match error {
            OutboundFailure::ConnectionClosed => {
                // ConnectionClosed is allowed here because we mainly test the behavior
                // of `server_task`.
            }
            OutboundFailure::Io(e) if e.kind() == io::ErrorKind::UnexpectedEof => {}
            e => panic!("Unexpected error: {e:?}"),
        };

        // Keep alive the task, so only `server_task` can finish
        wait_no_events(&mut swarm2).await;
    };

    let server_task = pin!(server_task);
    let client_task = pin!(client_task);
    futures::future::select(server_task, client_task).await;
}

#[async_std::test]
async fn report_inbound_timeout_on_write_response() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    // `swarm2` needs to have a bigger timeout to avoid racing
    let (peer1_id, mut swarm1) = new_swarm_with_timeout(Duration::from_millis(100));
    let (peer2_id, mut swarm2) = new_swarm_with_timeout(Duration::from_millis(200));

    swarm1.listen().with_memory_addr_external().await;
    swarm2.connect(&mut swarm1).await;

    // Expects InboundFailure::Timeout
    let server_task = async move {
        let (peer, req_id, action, resp_channel) = wait_request(&mut swarm1).await.unwrap();
        assert_eq!(peer, peer2_id);
        assert_eq!(action, Action::TimeoutOnWriteResponse);
        swarm1
            .behaviour_mut()
            .send_response(resp_channel, Action::TimeoutOnWriteResponse)
            .unwrap();

        let (peer, req_id_done, error) = wait_inbound_failure(&mut swarm1).await.unwrap();
        assert_eq!(peer, peer2_id);
        assert_eq!(req_id_done, req_id);
        assert!(matches!(error, InboundFailure::Timeout));
    };

    // Expects OutboundFailure::ConnectionClosed or io::ErrorKind::UnexpectedEof
    let client_task = async move {
        let req_id = swarm2
            .behaviour_mut()
            .send_request(&peer1_id, Action::TimeoutOnWriteResponse);

        let (peer, req_id_done, error) = wait_outbound_failure(&mut swarm2).await.unwrap();
        assert_eq!(peer, peer1_id);
        assert_eq!(req_id_done, req_id);

        match error {
            OutboundFailure::ConnectionClosed => {
                // ConnectionClosed is allowed here because we mainly test the behavior
                // of `server_task`.
            }
            OutboundFailure::Io(e) if e.kind() == io::ErrorKind::UnexpectedEof => {}
            e => panic!("Unexpected error: {e:?}"),
        }

        // Keep alive the task, so only `server_task` can finish
        wait_no_events(&mut swarm2).await;
    };

    let server_task = pin!(server_task);
    let client_task = pin!(client_task);
    futures::future::select(server_task, client_task).await;
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
    FailOnMaxStreams,
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
            Action::FailOnMaxStreams => 6,
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
            6 => Ok(Action::FailOnMaxStreams),
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

fn new_swarm_with_config(
    cfg: request_response::Config,
) -> (PeerId, Swarm<request_response::Behaviour<TestCodec>>) {
    let protocols = iter::once((StreamProtocol::new("/test/1"), ProtocolSupport::Full));

    let swarm =
        Swarm::new_ephemeral(|_| request_response::Behaviour::<TestCodec>::new(protocols, cfg));
    let peed_id = *swarm.local_peer_id();

    (peed_id, swarm)
}

fn new_swarm_with_timeout(
    timeout: Duration,
) -> (PeerId, Swarm<request_response::Behaviour<TestCodec>>) {
    let cfg = request_response::Config::default().with_request_timeout(timeout);
    new_swarm_with_config(cfg)
}

fn new_swarm() -> (PeerId, Swarm<request_response::Behaviour<TestCodec>>) {
    new_swarm_with_timeout(Duration::from_millis(100))
}

async fn wait_no_events(swarm: &mut Swarm<request_response::Behaviour<TestCodec>>) {
    loop {
        if let Ok(ev) = swarm.select_next_some().await.try_into_behaviour_event() {
            panic!("Unexpected event: {ev:?}")
        }
    }
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
                ..
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

async fn wait_inbound_failure(
    swarm: &mut Swarm<request_response::Behaviour<TestCodec>>,
) -> Result<(PeerId, InboundRequestId, InboundFailure)> {
    loop {
        match swarm.select_next_some().await.try_into_behaviour_event() {
            Ok(request_response::Event::InboundFailure {
                peer,
                request_id,
                error,
                ..
            }) => {
                return Ok((peer, request_id, error));
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
                ..
            }) => {
                return Ok((peer, request_id, error));
            }
            Ok(ev) => bail!("Unexpected event: {ev:?}"),
            Err(..) => {}
        }
    }
}
