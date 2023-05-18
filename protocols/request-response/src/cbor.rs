// Copyright 2020 Parity Technologies (UK) Ltd.
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

use crate::{Config, ProtocolSupport};
use async_trait::async_trait;
use futures::prelude::*;
use futures::{AsyncRead, AsyncWrite};
use libp2p_core::upgrade::{read_length_prefixed, write_length_prefixed};
use libp2p_swarm::{NetworkBehaviour, StreamProtocol};
use serde::{de::DeserializeOwned, Serialize};
use std::{io, marker::PhantomData};

#[derive(Debug, Clone)]
pub struct Codec<Req, Resp> {
    phantom: PhantomData<(Req, Resp)>,
}

const REQUEST_SIZE_MAXIMUM: usize = 1_000_000;
const RESPONSE_SIZE_MAXIMUM: usize = 500_000_000;

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
    inner: crate::Behaviour<Codec<Req, Resp>>,
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
        let vec = read_length_prefixed(io, REQUEST_SIZE_MAXIMUM).await?;

        if vec.is_empty() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }

        Ok(serde_json::from_slice(vec.as_slice())?)
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let vec = read_length_prefixed(io, RESPONSE_SIZE_MAXIMUM).await?;

        if vec.is_empty() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }

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
        write_length_prefixed(io, data).await?;
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
        write_length_prefixed(io, data).await?;
        io.close().await?;

        Ok(())
    }
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
                <Codec<Req, Resp> as crate::Codec>::Protocol,
                ProtocolSupport,
            ),
        >,
    {
        Behaviour {
            inner: crate::Behaviour::new(
                Codec {
                    phantom: PhantomData,
                },
                protocols,
                cfg,
            ),
        }
    }
}
