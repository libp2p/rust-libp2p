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

use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

async fn send_receive<S: AsyncRead + AsyncWrite + Unpin>(
    to_send: usize,
    to_receive: usize,
    mut stream: S,
) -> Result<(), std::io::Error> {
    stream.write_all(vec![0; to_send].as_slice()).await?;

    let mut buf = Vec::with_capacity(to_receive);
    stream.read_to_end(&mut buf).await?;

    Ok(())
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

        block_on(send_receive(0, 0, stream.clone())).unwrap();

        assert_eq!(stream.inner.lock().unwrap().write, vec![]);
    }
}
