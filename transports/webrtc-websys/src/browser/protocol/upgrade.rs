use std::{future::Future, pin::Pin};

use futures::{AsyncRead, AsyncWrite};
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p_swarm::StreamProtocol;

use crate::browser::{stream::SignalingStream, SIGNALING_STREAM_PROTOCOL};

pub struct SignalingProtocolUpgrade;

impl UpgradeInfo for SignalingProtocolUpgrade {
    type Info = StreamProtocol;
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(SIGNALING_STREAM_PROTOCOL)
    }
}

impl<T> InboundUpgrade<T> for SignalingProtocolUpgrade
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = SignalingStream<T>;
    type Error = std::io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, socket: T, _info: Self::Info) -> Self::Future {
        Box::pin(async move { Ok(SignalingStream::new(socket)) })
    }
}

impl<T> OutboundUpgrade<T> for SignalingProtocolUpgrade
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = SignalingStream<T>;
    type Error = std::io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, socket: T, _info: Self::Info) -> Self::Future {
        Box::pin(async move { Ok(SignalingStream::new(socket)) })
    }
}
