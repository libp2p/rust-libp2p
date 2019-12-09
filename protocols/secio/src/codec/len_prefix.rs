// Copyright 2019 Parity Technologies (UK) Ltd.
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

use futures::{prelude::*, stream::BoxStream};
use quicksink::Action;
use std::{fmt, io, pin::Pin, task::{Context, Poll}};

/// `Stream` & `Sink` that reads and writes a length prefix in front of the actual data.
pub struct LenPrefixCodec<T> {
    stream: BoxStream<'static, io::Result<Vec<u8>>>,
    sink: Pin<Box<dyn Sink<Vec<u8>, Error = io::Error> + Send>>,
    _mark: std::marker::PhantomData<T>
}

impl<T> fmt::Debug for LenPrefixCodec<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("LenPrefixCodec")
    }
}

static_assertions::const_assert! {
    std::mem::size_of::<u32>() <= std::mem::size_of::<usize>()
}

impl<T> LenPrefixCodec<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static
{
    pub fn new(socket: T, max_len: usize) -> Self {
        let (r, w) = socket.split();

        let stream = futures::stream::unfold(r, move |mut r| async move {
            let mut len = [0; 4];
            if let Err(e) = r.read_exact(&mut len).await {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    return None
                }
                return Some((Err(e), r))
            }
            let n = u32::from_be_bytes(len) as usize;
            if n > max_len {
                let msg = format!("data length {} exceeds allowed maximum {}", n, max_len);
                return Some((Err(io::Error::new(io::ErrorKind::PermissionDenied, msg)), r))
            }
            let mut v = vec![0; n];
            if let Err(e) = r.read_exact(&mut v).await {
                return Some((Err(e), r))
            }
            Some((Ok(v), r))
        });

        let sink = quicksink::make_sink(w, move |mut w, action: Action<Vec<u8>>| async move {
            match action {
                Action::Send(data) => {
                    if data.len() > max_len {
                        log::error!("data length {} exceeds allowed maximum {}", data.len(), max_len)
                    }
                    w.write_all(&(data.len() as u32).to_be_bytes()).await?;
                    w.write_all(&data).await?
                }
                Action::Flush => w.flush().await?,
                Action::Close => w.close().await?
            }
            Ok(w)
        });

        LenPrefixCodec {
            stream: stream.boxed(),
            sink: Box::pin(sink),
            _mark: std::marker::PhantomData
        }
    }
}

impl<T> Stream for LenPrefixCodec<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static
{
    type Item = io::Result<Vec<u8>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.stream.poll_next_unpin(cx)
    }
}

impl<T> Sink<Vec<u8>> for LenPrefixCodec<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static
{
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.sink).poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Vec<u8>) -> Result<(), Self::Error> {
        Pin::new(&mut self.sink).start_send(item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.sink).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.sink).poll_close(cx)
    }
}
