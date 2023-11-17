//! After a successful protocol negotiation as part of the upgrade process, the `SecurityUpgrade::upgrade_security`
//! method is called and a [`Future`] that performs a handshake is returned.

use std::iter::IntoIterator;

use crate::upgrade::UpgradeError;
use crate::UpgradeInfo;
use crate::{connection::ConnectedPoint, Negotiated};
use futures::future::{BoxFuture, Either};
use futures::prelude::*;

use libp2p_identity::PeerId;
use multiaddr::Protocol;
use multistream_select::Version;

use super::{InboundSecurityUpgrade, OutboundSecurityUpgrade};

/// An inbound or outbound security upgrade.
pub(crate) type EitherSecurityFuture<C, U> =
    Either<InboundSecurityFuture<C, U>, OutboundSecurityFuture<C, U>>;

/// An inbound security upgrade represented by an owned trait object `Future`.
pub(crate) type InboundSecurityFuture<C, U> = BoxFuture<
    'static,
    Result<
        (PeerId, <U as InboundSecurityUpgrade<Negotiated<C>>>::Output),
        UpgradeError<<U as InboundSecurityUpgrade<Negotiated<C>>>::Error>,
    >,
>;

/// An outbound security upgrade represented by an owned trait object `Future`.
pub(crate) type OutboundSecurityFuture<C, U> = BoxFuture<
    'static,
    Result<
        (
            PeerId,
            <U as OutboundSecurityUpgrade<Negotiated<C>>>::Output,
        ),
        UpgradeError<<U as OutboundSecurityUpgrade<Negotiated<C>>>::Error>,
    >,
>;

/// Applies a security upgrade to the inbound and outbound direction of a connection or substream.
pub(crate) fn secure<C, U>(
    conn: C,
    up: U,
    cp: ConnectedPoint,
    v: Version,
) -> EitherSecurityFuture<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    U: InboundSecurityUpgrade<Negotiated<C>>
        + OutboundSecurityUpgrade<Negotiated<C>>
        + Send
        + 'static,
    <U as UpgradeInfo>::Info: Send,
    <U as InboundSecurityUpgrade<Negotiated<C>>>::Future: Send,
    <U as OutboundSecurityUpgrade<Negotiated<C>>>::Future: Send,
    <<U as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send,
{
    match cp {
        ConnectedPoint::Dialer { role_override, .. } if role_override.is_dialer() => Either::Right(
            async move {
                let peer_id = cp
                    .get_remote_address()
                    .iter()
                    .find_map(|protocol| match protocol {
                        Protocol::P2p(peer_id) => Some(peer_id),
                        _ => None,
                    });
                let (info, stream) =
                    multistream_select::dialer_select_proto(conn, up.protocol_info(), v).await?;
                let name = info.as_ref().to_owned();
                match up.secure_outbound(stream, info, peer_id).await {
                    Ok(x) => {
                        tracing::trace!(up=%name, "Secured outbound stream");
                        Ok(x)
                    }
                    Err(e) => {
                        tracing::trace!(up=%name, "Failed to secure outbound stream");
                        Err(UpgradeError::Apply(e))
                    }
                }
            }
            .boxed(),
        ),
        _ => Either::Left(
            async move {
                let (info, stream) =
                    multistream_select::listener_select_proto(conn, up.protocol_info()).await?;
                let name = info.as_ref().to_owned();
                match up.secure_inbound(stream, info, None).await {
                    Ok(x) => {
                        tracing::trace!(up=%name, "Secured inbound stream");
                        Ok(x)
                    }
                    Err(e) => {
                        tracing::trace!(up=%name, "Failed to secure inbound stream");
                        Err(UpgradeError::Apply(e))
                    }
                }
            }
            .boxed(),
        ),
    }
}
