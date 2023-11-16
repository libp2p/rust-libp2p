//! After a successful protocol negotiation as part of the upgrade process, the `SecurityUpgrade::upgrade_security`
//! method is called and a [`Future`] that performs a handshake is returned.

use crate::upgrade::{SecurityUpgrade, UpgradeError};
use crate::{connection::ConnectedPoint, Negotiated};
use futures::prelude::*;

use libp2p_identity::PeerId;
use multiaddr::Protocol;
use multistream_select::Version;

/// Applies a security upgrade to the inbound and outbound direction of a connection or substream.
pub(crate) async fn secure<C, U>(
    conn: C,
    up: U,
    cp: ConnectedPoint,
    v: Version,
) -> Result<(PeerId, U::Output), UpgradeError<U::Error>>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: SecurityUpgrade<Negotiated<C>>,
{
    match cp {
        ConnectedPoint::Dialer { role_override, .. } if role_override.is_dialer() => {
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
            match up.upgrade_security(stream, info, peer_id).await {
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
        _ => {
            let (info, stream) =
                multistream_select::listener_select_proto(conn, up.protocol_info()).await?;
            let name = info.as_ref().to_owned();
            match up.upgrade_security(stream, info, None).await {
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
    }
}
