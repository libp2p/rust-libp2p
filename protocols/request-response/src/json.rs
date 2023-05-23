// Copyright 2023 Protocol Labs
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use crate::{Config, ProtocolSupport, RequestId, ResponseChannel};
use libp2p_core::Multiaddr;
use libp2p_identity::PeerId;
use libp2p_swarm::NetworkBehaviour;
use serde::{de::DeserializeOwned, Serialize};

pub type OutEvent<Req, Resp> = crate::Event<Req, Resp>;

#[derive(NetworkBehaviour)]
#[behaviour(
    to_swarm = "OutEvent<Req, Resp>",
    prelude = "libp2p_swarm::derive_prelude"
)]
pub struct Behaviour<Req, Resp>
where
    Req: Send + Clone + Serialize + DeserializeOwned + 'static,
    Resp: Send + Clone + Serialize + DeserializeOwned + 'static,
{
    inner: crate::Behaviour<codec::Codec<Req, Resp>>,
}

impl<Req, Resp> Behaviour<Req, Resp>
where
    Req: Send + Clone + Serialize + DeserializeOwned,
    Resp: Send + Clone + Serialize + DeserializeOwned,
{
    pub fn new<I>(protocols: I, cfg: Config) -> Self
    where
        I: IntoIterator<
            Item = (
                <codec::Codec<Req, Resp> as crate::Codec>::Protocol,
                ProtocolSupport,
            ),
        >,
    {
        Behaviour {
            inner: crate::Behaviour::new(codec::Codec::default(), protocols, cfg),
        }
    }

    pub fn send_request(&mut self, peer: &PeerId, request: Req) -> RequestId {
        self.inner.send_request(peer, request)
    }

    pub fn send_response(&mut self, ch: ResponseChannel<Resp>, rs: Resp) -> Result<(), Resp> {
        self.inner.send_response(ch, rs)
    }

    pub fn add_address(&mut self, peer: &PeerId, address: Multiaddr) {
        self.inner.add_address(peer, address)
    }

    pub fn remove_address(&mut self, peer: &PeerId, address: &Multiaddr) {
        self.inner.remove_address(peer, address)
    }

    pub fn is_connected(&self, peer: &PeerId) -> bool {
        self.inner.is_connected(peer)
    }

    pub fn is_pending_outbound(&self, peer: &PeerId, request_id: &RequestId) -> bool {
        self.inner.is_pending_outbound(peer, request_id)
    }

    pub fn is_pending_inbound(&self, peer: &PeerId, request_id: &RequestId) -> bool {
        self.inner.is_pending_inbound(peer, request_id)
    }
}

mod codec {
    use async_trait::async_trait;
    use futures::prelude::*;
    use futures::{AsyncRead, AsyncWrite};
    use libp2p_core::upgrade::read_to_end;
    use libp2p_swarm::StreamProtocol;
    use serde::{de::DeserializeOwned, Serialize};
    use std::{io, marker::PhantomData};

    /// Max request size in bytes
    const REQUEST_SIZE_MAXIMUM: usize = 1024 * 1024;
    /// Max response size in bytes
    const RESPONSE_SIZE_MAXIMUM: usize = 10 * 1024 * 1024;

    #[derive(Debug, Clone)]
    pub struct Codec<Req, Resp> {
        phantom: PhantomData<(Req, Resp)>,
    }

    impl<Req, Resp> Default for Codec<Req, Resp> {
        fn default() -> Self {
            Codec {
                phantom: PhantomData,
            }
        }
    }

    #[async_trait]
    impl<Req, Resp> crate::Codec for Codec<Req, Resp>
    where
        Req: Send + Clone + Serialize + DeserializeOwned,
        Resp: Send + Clone + Serialize + DeserializeOwned,
    {
        type Protocol = StreamProtocol;
        type Request = Req;
        type Response = Resp;

        async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Req>
        where
            T: AsyncRead + Unpin + Send,
        {
            let vec = read_to_end(io, REQUEST_SIZE_MAXIMUM).await?;

            Ok(serde_json::from_slice(vec.as_slice())?)
        }

        async fn read_response<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Resp>
        where
            T: AsyncRead + Unpin + Send,
        {
            let vec = read_to_end(io, RESPONSE_SIZE_MAXIMUM).await?;

            Ok(serde_json::from_slice(vec.as_slice())?)
        }

        async fn write_request<T>(
            &mut self,
            _: &Self::Protocol,
            io: &mut T,
            req: Self::Request,
        ) -> io::Result<()>
        where
            T: AsyncWrite + Unpin + Send,
        {
            let data = serde_json::to_vec(&req)?;

            io.write_all(data.as_ref()).await?;
            io.flush().await?;
            io.close().await?;

            Ok(())
        }

        async fn write_response<T>(
            &mut self,
            _: &Self::Protocol,
            io: &mut T,
            resp: Self::Response,
        ) -> io::Result<()>
        where
            T: AsyncWrite + Unpin + Send,
        {
            let data = serde_json::to_vec(&resp)?;
            io.write_all(data.as_ref()).await?;
            io.flush().await?;
            io.close().await?;

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Codec;
    use futures_ringbuf::Endpoint;
    use libp2p_swarm::StreamProtocol;
    use serde::{Deserialize, Serialize};

    #[async_std::test]
    async fn test_codec() {
        let expected_request = TestRequest {
            payload: "test_payload".to_string(),
        };
        let expected_response = TestResponse {
            payload: "test_payload".to_string(),
        };
        let protocol = StreamProtocol::new("/test_cbor/1");
        let mut codec: super::codec::Codec<TestRequest, TestResponse> =
            super::codec::Codec::default();

        let (mut a, mut b) = Endpoint::pair(124, 124);
        codec
            .write_request(&protocol, &mut a, expected_request.clone())
            .await
            .expect("Should write request");
        let actual_request = codec
            .read_request(&protocol, &mut b)
            .await
            .expect("Should read request");

        assert_eq!(actual_request, expected_request);

        let (mut a, mut b) = Endpoint::pair(124, 124);
        codec
            .write_response(&protocol, &mut a, expected_response.clone())
            .await
            .expect("Should write response");
        let actual_response = codec
            .read_response(&protocol, &mut b)
            .await
            .expect("Should read response");

        assert_eq!(actual_response, expected_response);
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestRequest {
        payload: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestResponse {
        payload: String,
    }
}
