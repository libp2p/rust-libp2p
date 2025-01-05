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

/// A request-response behaviour using [`serde_json`] for serializing and deserializing the
/// messages.
///
/// # Example
///
/// ```
/// # use libp2p_request_response::{json, ProtocolSupport, self as request_response};
/// # use libp2p_swarm::{StreamProtocol};
/// #[derive(Debug, serde::Serialize, serde::Deserialize)]
/// struct GreetRequest {
///     name: String,
/// }
///
/// #[derive(Debug, serde::Serialize, serde::Deserialize)]
/// struct GreetResponse {
///     message: String,
/// }
///
/// let behaviour = json::Behaviour::<GreetRequest, GreetResponse>::new(
///     [(
///         StreamProtocol::new("/my-json-protocol"),
///         ProtocolSupport::Full,
///     )],
///     request_response::Config::default(),
/// );
/// ```
pub type Behaviour<Req, Resp> = crate::Behaviour<codec::Codec<Req, Resp>>;

mod codec {
    use std::{io, marker::PhantomData};

    use async_trait::async_trait;
    use futures::prelude::*;
    use libp2p_swarm::StreamProtocol;
    use serde::{de::DeserializeOwned, Serialize};

    pub struct Codec<Req, Resp> {
        /// Max request size in bytes
        request_size_maximum: u64,
        /// Max response size in bytes
        response_size_maximum: u64,
        phantom: PhantomData<(Req, Resp)>,
    }

    impl<Req, Resp> Default for Codec<Req, Resp> {
        fn default() -> Self {
            Codec {
                request_size_maximum: 1024 * 1024,
                response_size_maximum: 10 * 1024 * 1024,
                phantom: PhantomData,
            }
        }
    }

    impl<Req, Resp> Clone for Codec<Req, Resp> {
        fn clone(&self) -> Self {
            Self {
                request_size_maximum: self.request_size_maximum,
                response_size_maximum: self.response_size_maximum,
                phantom: self.phantom,
            }
        }
    }

    impl<Req, Resp> Codec<Req, Resp> {
        /// Sets the limit for request size in bytes.
        pub fn set_request_size_maximum(mut self, request_size_maximum: u64) -> Self {
            self.request_size_maximum = request_size_maximum;
            self
        }

        /// Sets the limit for response size in bytes.
        pub fn set_response_size_maximum(mut self, response_size_maximum: u64) -> Self {
            self.response_size_maximum = response_size_maximum;
            self
        }
    }

    #[async_trait]
    impl<Req, Resp> crate::Codec for Codec<Req, Resp>
    where
        Req: Send + Serialize + DeserializeOwned,
        Resp: Send + Serialize + DeserializeOwned,
    {
        type Protocol = StreamProtocol;
        type Request = Req;
        type Response = Resp;

        async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Req>
        where
            T: AsyncRead + Unpin + Send,
        {
            let mut vec = Vec::new();

            io.take(self.request_size_maximum)
                .read_to_end(&mut vec)
                .await?;

            Ok(serde_json::from_slice(vec.as_slice())?)
        }

        async fn read_response<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Resp>
        where
            T: AsyncRead + Unpin + Send,
        {
            let mut vec = Vec::new();

            io.take(self.response_size_maximum)
                .read_to_end(&mut vec)
                .await?;

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

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::AsyncWriteExt;
    use futures_ringbuf::Endpoint;
    use libp2p_swarm::StreamProtocol;
    use serde::{Deserialize, Serialize};

    use crate::Codec;

    #[async_std::test]
    async fn test_codec() {
        let expected_request = TestRequest {
            payload: "test_payload".to_string(),
        };
        let expected_response = TestResponse {
            payload: "test_payload".to_string(),
        };
        let protocol = StreamProtocol::new("/test_json/1");
        let mut codec: super::codec::Codec<TestRequest, TestResponse> =
            super::codec::Codec::default();

        let (mut a, mut b) = Endpoint::pair(124, 124);
        codec
            .write_request(&protocol, &mut a, expected_request.clone())
            .await
            .expect("Should write request");
        a.close().await.unwrap();

        let actual_request = codec
            .read_request(&protocol, &mut b)
            .await
            .expect("Should read request");
        b.close().await.unwrap();

        assert_eq!(actual_request, expected_request);

        let (mut a, mut b) = Endpoint::pair(124, 124);
        codec
            .write_response(&protocol, &mut a, expected_response.clone())
            .await
            .expect("Should write response");
        a.close().await.unwrap();

        let actual_response = codec
            .read_response(&protocol, &mut b)
            .await
            .expect("Should read response");
        b.close().await.unwrap();

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
