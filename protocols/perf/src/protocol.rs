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
use instant::Instant;
use libp2p_request_response as request_response;
use libp2p_swarm::StreamProtocol;
use std::io;

use crate::{RunDuration, RunParams};

const BUF: [u8; 65536] = [0; 64 << 10];

#[derive(Debug)]
pub enum Response {
    Sender(usize),
    Receiver(RunDuration),
}

#[derive(Default)]
pub struct Codec {
    to_receive: Option<usize>,

    write_start: Option<Instant>,
    read_start: Option<Instant>,
    read_done: Option<Instant>,
}

impl Clone for Codec {
    fn clone(&self) -> Self {
        Default::default()
    }
}

#[async_trait]
impl request_response::Codec for Codec {
    /// The type of protocol(s) or protocol versions being negotiated.
    type Protocol = StreamProtocol;
    /// The type of inbound and outbound requests.
    type Request = RunParams;
    /// The type of inbound and outbound responses.
    type Response = Response;

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
        assert!(self.write_start.is_some());
        assert_eq!(self.read_start, None);
        assert_eq!(self.read_done, None);

        self.read_start = Some(Instant::now());

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

        self.read_done = Some(Instant::now());

        assert_eq!(received, self.to_receive.unwrap());

        Ok(Response::Receiver(RunDuration {
            upload: self
                .read_start
                .unwrap()
                .duration_since(self.write_start.unwrap()),
            download: self
                .read_done
                .unwrap()
                .duration_since(self.read_start.unwrap()),
        }))
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
        assert_eq!(self.to_receive, None);
        assert_eq!(self.write_start, None);
        assert_eq!(self.read_start, None);
        assert_eq!(self.read_done, None);

        self.write_start = Some(Instant::now());

        let RunParams {
            to_send,
            to_receive,
        } = req;

        self.to_receive = Some(to_receive);

        io.write_all(&(to_receive as u64).to_be_bytes()).await?;

        let mut sent = 0;
        while sent < to_send {
            let n = std::cmp::min(to_send - sent, BUF.len());
            let buf = &BUF[..n];

            sent += io.write(buf).await?;
        }

        Ok(())
    }

    /// Writes a response to the given I/O stream according to the
    /// negotiated protocol.
    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        response: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let to_send = match response {
            Response::Sender(to_send) => to_send,
            Response::Receiver(_) => unreachable!(),
        };

        let mut sent = 0;
        while sent < to_send {
            let n = std::cmp::min(to_send - sent, BUF.len());
            let buf = &BUF[..n];

            sent += io.write(buf).await?;
        }

        Ok(())
    }
}
