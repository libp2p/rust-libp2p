//! The `/dg/1` control stream ([libp2p/specs#680]): the initiator sends the
//! length-prefixed application protocol id, then it stays open for the flow.
//!
//! [libp2p/specs#680]: https://github.com/libp2p/specs/pull/680

use std::{io, iter};

use futures::{AsyncReadExt, AsyncWriteExt, future::BoxFuture, prelude::*};
use libp2p_core::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p_swarm::{Stream, StreamProtocol};

pub(crate) const CONTROL_PROTOCOL: StreamProtocol = StreamProtocol::new("/dg/1");

const MAX_PROTOCOL_LEN: usize = 256;

/// Control-stream upgrade. Public only to satisfy the connection-handler signature.
pub struct Upgrade {
    pub(crate) application_protocol: StreamProtocol,
}

impl UpgradeInfo for Upgrade {
    type Info = StreamProtocol;
    type InfoIter = iter::Once<StreamProtocol>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(CONTROL_PROTOCOL)
    }
}

impl OutboundUpgrade<Stream> for Upgrade {
    type Output = Stream;
    type Error = io::Error;
    type Future = BoxFuture<'static, Result<Stream, io::Error>>;

    fn upgrade_outbound(self, mut stream: Stream, _: StreamProtocol) -> Self::Future {
        async move {
            let id = self.application_protocol.as_ref().as_bytes();
            let mut len = unsigned_varint::encode::usize_buffer();
            stream
                .write_all(unsigned_varint::encode::usize(id.len(), &mut len))
                .await?;
            stream.write_all(id).await?;
            stream.flush().await?;
            Ok(stream)
        }
        .boxed()
    }
}

impl InboundUpgrade<Stream> for Upgrade {
    type Output = Stream;
    type Error = io::Error;
    type Future = BoxFuture<'static, Result<Stream, io::Error>>;

    fn upgrade_inbound(self, mut stream: Stream, _: StreamProtocol) -> Self::Future {
        async move {
            let len = unsigned_varint::aio::read_usize(&mut stream)
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if len > MAX_PROTOCOL_LEN {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "protocol id too long",
                ));
            }
            let mut id = vec![0u8; len];
            stream.read_exact(&mut id).await?;
            if id != self.application_protocol.as_ref().as_bytes() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unexpected application protocol",
                ));
            }
            Ok(stream)
        }
        .boxed()
    }
}
