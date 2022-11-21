/// Simple message exchange protocol for MyChat
use std::io;

use async_trait::async_trait;
use libp2p::bytes::Bytes;
use libp2p::core::upgrade::{read_length_prefixed, write_length_prefixed};
use libp2p::futures::{AsyncRead, AsyncWrite, AsyncWriteExt};
use libp2p::request_response::{ProtocolName, RequestResponseCodec};

/// This is the maximum size in bytes of the message that we accept. This is
/// necessary in order to avoid DoS attacks where the remote sends us huge messages repeatedly.
pub const MAX_DIRECT_MESSAGE_ACCEPT_SIZE: usize = 1_000_000;

/// This request response protocol for the MyChat network is used to send direct messages between peers.
///
/// The protocol can be used to send any bytes between peers, as long as the message size is not
/// greater than MAX_DIRECT_MESSAGE_ACCEPT_SIZE.
///
/// If a peer does not respond to a message, a specific event is raised.
/// In the case of MyChat, this protocol is used to send one-way messages to peers, expecting only
/// a 0-byte in the immediate response. This confirms delivery. Peers respond with actual content
/// in later, async messages using this same protocol.
#[derive(Debug, Clone)]
pub struct MyChatMessageExchangeProtocol();
#[derive(Clone)]
pub struct MyChatMessageExchangeCodec();
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MyChatMessageRequest(pub Bytes);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MyChatMessageResponse(pub Bytes);

pub const ACK_MESSAGE: &[u8; 1] = &[0];
const DM_PROTO_NAME: &[u8] = b"/my-chat/dm/1.0.0";

impl ProtocolName for MyChatMessageExchangeProtocol {
    fn protocol_name(&self) -> &[u8] {
        DM_PROTO_NAME
    }
}

#[async_trait]
impl RequestResponseCodec for MyChatMessageExchangeCodec {
    type Protocol = MyChatMessageExchangeProtocol;
    type Request = MyChatMessageRequest;
    type Response = MyChatMessageResponse;

    async fn read_request<T>(
        &mut self,
        _: &MyChatMessageExchangeProtocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let vec = read_length_prefixed(io, MAX_DIRECT_MESSAGE_ACCEPT_SIZE).await?;

        if vec.is_empty() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }

        Ok(MyChatMessageRequest(vec.into()))
    }

    async fn read_response<T>(
        &mut self,
        _: &MyChatMessageExchangeProtocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let vec = read_length_prefixed(io, MAX_DIRECT_MESSAGE_ACCEPT_SIZE).await?;

        if vec.is_empty() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }

        Ok(MyChatMessageResponse(vec.into()))
    }

    async fn write_request<T>(
        &mut self,
        _: &MyChatMessageExchangeProtocol,
        io: &mut T,
        MyChatMessageRequest(data): MyChatMessageRequest,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_length_prefixed(io, data).await?;
        io.close().await?;

        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _: &MyChatMessageExchangeProtocol,
        io: &mut T,
        MyChatMessageResponse(data): MyChatMessageResponse,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_length_prefixed(io, data).await?;
        io.close().await?;

        Ok(())
    }
}
