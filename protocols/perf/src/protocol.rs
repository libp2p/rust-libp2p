// Copyright 2023 Protocol Labs.
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

use async_trait::async_trait;
use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use libp2p_request_response as request_response;
use std::io;

use crate::RunParams;

const BUF: [u8; 65536] = [0; 64 << 10];

#[derive(Clone)]
pub struct Codec {}

#[async_trait]
impl request_response::Codec for Codec {
    /// The type of protocol(s) or protocol versions being negotiated.
    type Protocol = &'static [u8; 11];
    /// The type of inbound and outbound requests.
    type Request = RunParams;
    /// The type of inbound and outbound responses.
    type Response = usize;

    /// Reads a request from the given I/O stream according to the
    /// negotiated protocol.
    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut receive_buf = vec![0; 64 << 10];

        let to_send = {
            let mut buf = [0; 8];
            io.read_exact(&mut buf).await?;

            u64::from_be_bytes(buf) as usize
        };

        let mut received = 0;
        loop {
            let n = io.read(&mut receive_buf).await?;
            received += n;
            if n == 0 {
                break;
            }
        }

        Ok(RunParams {
            to_receive: received,
            to_send,
        })
    }

    /// Reads a response from the given I/O stream according to the
    /// negotiated protocol.
    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut receive_buf = vec![0; 64 << 10];

        let mut received = 0;
        loop {
            let n = io.read(&mut receive_buf).await?;
            received += n;
            // Make sure to wait for the remote to close the stream. Otherwise with `to_receive` of `0`
            // one does not measure the full round-trip of the previous write.
            if n == 0 {
                break;
            }
        }

        Ok(received)
    }

    /// Writes a request to the given I/O stream according to the
    /// negotiated protocol.
    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let RunParams {
            to_send,
            to_receive,
        } = req;

        io.write_all(&(to_receive as u64).to_be_bytes()).await?;

        let mut sent = 0;
        while sent < to_send {
            let n = std::cmp::min(to_send - sent, BUF.len());
            let buf = &BUF[..n];

            sent += io.write(buf).await?;
        }

        io.close().await?;

        Ok(())
    }

    /// Writes a response to the given I/O stream according to the
    /// negotiated protocol.
    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        to_send: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let mut sent = 0;
        while sent < to_send {
            let n = std::cmp::min(to_send - sent, BUF.len());
            let buf = &BUF[..n];

            sent += io.write(buf).await?;
        }

        io.close().await?;

        Ok(())
    }
}
