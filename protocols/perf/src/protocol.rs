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

use instant::Instant;

use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{client, server};

const BUF: [u8; 1024] = [0; 1024];

pub async fn send_receive<S: AsyncRead + AsyncWrite + Unpin>(
    params: client::RunParams,
    mut stream: S,
) -> Result<client::RunTimers, std::io::Error> {
    let client::RunParams {
        to_send,
        to_receive,
    } = params;

    let mut receive_buf = vec![0; 1024];

    stream.write_all(&(to_receive as u64).to_be_bytes()).await?;

    let write_start = Instant::now();

    let mut sent = 0;
    while sent < to_send {
        let n = std::cmp::min(to_send - sent, BUF.len());
        let buf = &BUF[..n];

        sent += stream.write(buf).await?;
    }

    stream.close().await?;

    let write_done = Instant::now();

    let mut received = 0;
    while received < to_receive {
        received += stream.read(&mut receive_buf).await?;
    }

    let read_done = Instant::now();

    Ok(client::RunTimers {
        write_start,
        write_done,
        read_done,
    })
}

pub async fn receive_send<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
) -> Result<server::RunStats, std::io::Error> {
    let to_send = {
        let mut buf = [0; 8];
        stream.read_exact(&mut buf).await?;

        u64::from_be_bytes(buf) as usize
    };

    let read_start = Instant::now();

    let mut receive_buf = vec![0; 1024];
    let mut received = 0;
    loop {
        let n = stream.read(&mut receive_buf).await?;
        received += n;
        if n == 0 {
            break;
        }
    }

    let read_done = Instant::now();

    let mut sent = 0;
    while sent < to_send {
        let n = std::cmp::min(to_send - sent, BUF.len());
        let buf = &BUF[..n];

        sent += stream.write(buf).await?;
    }

    stream.close().await?;
    let write_done = Instant::now();

    Ok(server::RunStats {
        params: server::RunParams { sent, received },
        timers: server::RunTimers {
            read_start,
            read_done,
            write_done,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{executor::block_on, AsyncRead, AsyncWrite};
    use std::{
        pin::Pin,
        sync::{Arc, Mutex},
        task::Poll,
    };

    #[derive(Clone)]
    struct DummyStream {
        inner: Arc<Mutex<DummyStreamInner>>,
    }

    struct DummyStreamInner {
        read: Vec<u8>,
        write: Vec<u8>,
    }

    impl DummyStream {
        fn new(read: Vec<u8>) -> Self {
            Self {
                inner: Arc::new(Mutex::new(DummyStreamInner {
                    read,
                    write: Vec::new(),
                })),
            }
        }
    }

    impl Unpin for DummyStream {}

    impl AsyncWrite for DummyStream {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            Pin::new(&mut self.inner.lock().unwrap().write).poll_write(cx, buf)
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner.lock().unwrap().write).poll_flush(cx)
        }

        fn poll_close(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner.lock().unwrap().write).poll_close(cx)
        }
    }

    impl AsyncRead for DummyStream {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &mut [u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            let amt = std::cmp::min(buf.len(), self.inner.lock().unwrap().read.len());
            let new = self.inner.lock().unwrap().read.split_off(amt);

            buf[..amt].copy_from_slice(self.inner.lock().unwrap().read.as_slice());

            self.inner.lock().unwrap().read = new;
            Poll::Ready(Ok(amt))
        }
    }

    #[test]
    fn test_client() {
        let stream = DummyStream::new(vec![0]);

        block_on(send_receive(
            client::RunParams {
                to_send: 0,
                to_receive: 0,
            },
            stream.clone(),
        ))
        .unwrap();

        assert_eq!(
            stream.inner.lock().unwrap().write,
            0u64.to_be_bytes().to_vec()
        );
    }
}
