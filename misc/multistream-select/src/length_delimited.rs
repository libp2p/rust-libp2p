// Copyright 2017 Parity Technologies (UK) Ltd.
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

use bytes::{Buf as _, BufMut as _, Bytes, BytesMut};
use futures::{io::IoSlice, prelude::*};
use std::{
    convert::TryFrom as _,
    io,
    pin::Pin,
    task::{Context, Poll},
    u16,
};

const MAX_LEN_BYTES: u16 = 2;
const MAX_FRAME_SIZE: u16 = (1 << (MAX_LEN_BYTES * 8 - MAX_LEN_BYTES)) - 1;
const DEFAULT_BUFFER_SIZE: usize = 64;

/// A `Stream` and `Sink` for unsigned-varint length-delimited frames,
/// wrapping an underlying `AsyncRead + AsyncWrite` I/O resource.
///
/// We purposely only support a frame sizes up to 16KiB (2 bytes unsigned varint
/// frame length). Frames mostly consist in a short protocol name, which is highly
/// unlikely to be more than 16KiB long.
#[pin_project::pin_project]
#[derive(Debug)]
pub struct LengthDelimited<R> {
    /// The inner I/O resource.
    #[pin]
    inner: R,
    /// Read buffer for a single incoming unsigned-varint length-delimited frame.
    read_buffer: BytesMut,
    /// Write buffer for outgoing unsigned-varint length-delimited frames.
    write_buffer: BytesMut,
    /// The current read state, alternating between reading a frame
    /// length and reading a frame payload.
    read_state: ReadState,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ReadState {
    /// We are currently reading the length of the next frame of data.
    ReadLength {
        buf: [u8; MAX_LEN_BYTES as usize],
        pos: usize,
    },
    /// We are currently reading the frame of data itself.
    ReadData { len: u16, pos: usize },
}

impl Default for ReadState {
    fn default() -> Self {
        ReadState::ReadLength {
            buf: [0; MAX_LEN_BYTES as usize],
            pos: 0,
        }
    }
}

impl<R> LengthDelimited<R> {
    /// Creates a new I/O resource for reading and writing unsigned-varint
    /// length delimited frames.
    pub fn new(inner: R) -> LengthDelimited<R> {
        LengthDelimited {
            inner,
            read_state: ReadState::default(),
            read_buffer: BytesMut::with_capacity(DEFAULT_BUFFER_SIZE),
            write_buffer: BytesMut::with_capacity(DEFAULT_BUFFER_SIZE + MAX_LEN_BYTES as usize),
        }
    }

    /// Drops the [`LengthDelimited`] resource, yielding the underlying I/O stream.
    ///
    /// # Panic
    ///
    /// Will panic if called while there is data in the read or write buffer.
    /// The read buffer is guaranteed to be empty whenever `Stream::poll` yields
    /// a new `Bytes` frame. The write buffer is guaranteed to be empty after
    /// flushing.
    pub fn into_inner(self) -> R {
        assert!(self.read_buffer.is_empty());
        assert!(self.write_buffer.is_empty());
        self.inner
    }

    /// Converts the [`LengthDelimited`] into a [`LengthDelimitedReader`], dropping the
    /// uvi-framed `Sink` in favour of direct `AsyncWrite` access to the underlying
    /// I/O stream.
    ///
    /// This is typically done if further uvi-framed messages are expected to be
    /// received but no more such messages are written, allowing the writing of
    /// follow-up protocol data to commence.
    pub fn into_reader(self) -> LengthDelimitedReader<R> {
        LengthDelimitedReader { inner: self }
    }

    /// Writes all buffered frame data to the underlying I/O stream,
    /// _without flushing it_.
    ///
    /// After this method returns `Poll::Ready`, the write buffer of frames
    /// submitted to the `Sink` is guaranteed to be empty.
    pub fn poll_write_buffer(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>>
    where
        R: AsyncWrite,
    {
        let mut this = self.project();

        while !this.write_buffer.is_empty() {
            match this.inner.as_mut().poll_write(cx, this.write_buffer) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "Failed to write buffered frame.",
                    )))
                }
                Poll::Ready(Ok(n)) => this.write_buffer.advance(n),
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            }
        }

        Poll::Ready(Ok(()))
    }
}

impl<R> Stream for LengthDelimited<R>
where
    R: AsyncRead,
{
    type Item = Result<Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        loop {
            match this.read_state {
                ReadState::ReadLength { buf, pos } => {
                    match this.inner.as_mut().poll_read(cx, &mut buf[*pos..*pos + 1]) {
                        Poll::Ready(Ok(0)) => {
                            if *pos == 0 {
                                return Poll::Ready(None);
                            } else {
                                return Poll::Ready(Some(Err(io::ErrorKind::UnexpectedEof.into())));
                            }
                        }
                        Poll::Ready(Ok(n)) => {
                            debug_assert_eq!(n, 1);
                            *pos += n;
                        }
                        Poll::Ready(Err(err)) => return Poll::Ready(Some(Err(err))),
                        Poll::Pending => return Poll::Pending,
                    };

                    if (buf[*pos - 1] & 0x80) == 0 {
                        // MSB is not set, indicating the end of the length prefix.
                        let (len, _) = unsigned_varint::decode::u16(buf).map_err(|e| {
                            log::debug!("invalid length prefix: {}", e);
                            io::Error::new(io::ErrorKind::InvalidData, "invalid length prefix")
                        })?;

                        if len >= 1 {
                            *this.read_state = ReadState::ReadData { len, pos: 0 };
                            this.read_buffer.resize(len as usize, 0);
                        } else {
                            debug_assert_eq!(len, 0);
                            *this.read_state = ReadState::default();
                            return Poll::Ready(Some(Ok(Bytes::new())));
                        }
                    } else if *pos == MAX_LEN_BYTES as usize {
                        // MSB signals more length bytes but we have already read the maximum.
                        // See the module documentation about the max frame len.
                        return Poll::Ready(Some(Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Maximum frame length exceeded",
                        ))));
                    }
                }
                ReadState::ReadData { len, pos } => {
                    match this
                        .inner
                        .as_mut()
                        .poll_read(cx, &mut this.read_buffer[*pos..])
                    {
                        Poll::Ready(Ok(0)) => {
                            return Poll::Ready(Some(Err(io::ErrorKind::UnexpectedEof.into())))
                        }
                        Poll::Ready(Ok(n)) => *pos += n,
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(Err(err)) => return Poll::Ready(Some(Err(err))),
                    };

                    if *pos == *len as usize {
                        // Finished reading the frame.
                        let frame = this.read_buffer.split_off(0).freeze();
                        *this.read_state = ReadState::default();
                        return Poll::Ready(Some(Ok(frame)));
                    }
                }
            }
        }
    }
}

impl<R> Sink<Bytes> for LengthDelimited<R>
where
    R: AsyncWrite,
{
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Use the maximum frame length also as a (soft) upper limit
        // for the entire write buffer. The actual (hard) limit is thus
        // implied to be roughly 2 * MAX_FRAME_SIZE.
        if self.as_mut().project().write_buffer.len() >= MAX_FRAME_SIZE as usize {
            match self.as_mut().poll_write_buffer(cx) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => return Poll::Pending,
            }

            debug_assert!(self.as_mut().project().write_buffer.is_empty());
        }

        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: Bytes) -> Result<(), Self::Error> {
        let this = self.project();

        let len = match u16::try_from(item.len()) {
            Ok(len) if len <= MAX_FRAME_SIZE => len,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Maximum frame size exceeded.",
                ))
            }
        };

        let mut uvi_buf = unsigned_varint::encode::u16_buffer();
        let uvi_len = unsigned_varint::encode::u16(len, &mut uvi_buf);
        this.write_buffer.reserve(len as usize + uvi_len.len());
        this.write_buffer.put(uvi_len);
        this.write_buffer.put(item);

        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Write all buffered frame data to the underlying I/O stream.
        match LengthDelimited::poll_write_buffer(self.as_mut(), cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Pending => return Poll::Pending,
        }

        let this = self.project();
        debug_assert!(this.write_buffer.is_empty());

        // Flush the underlying I/O stream.
        this.inner.poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Write all buffered frame data to the underlying I/O stream.
        match LengthDelimited::poll_write_buffer(self.as_mut(), cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Pending => return Poll::Pending,
        }

        let this = self.project();
        debug_assert!(this.write_buffer.is_empty());

        // Close the underlying I/O stream.
        this.inner.poll_close(cx)
    }
}

/// A `LengthDelimitedReader` implements a `Stream` of uvi-length-delimited
/// frames on an underlying I/O resource combined with direct `AsyncWrite` access.
#[pin_project::pin_project]
#[derive(Debug)]
pub struct LengthDelimitedReader<R> {
    #[pin]
    inner: LengthDelimited<R>,
}

impl<R> LengthDelimitedReader<R> {
    /// Destroys the `LengthDelimitedReader` and returns the underlying I/O stream.
    ///
    /// This method is guaranteed not to drop any data read from or not yet
    /// submitted to the underlying I/O stream.
    ///
    /// # Panic
    ///
    /// Will panic if called while there is data in the read or write buffer.
    /// The read buffer is guaranteed to be empty whenever [`Stream::poll_next`]
    /// yield a new `Message`. The write buffer is guaranteed to be empty whenever
    /// [`LengthDelimited::poll_write_buffer`] yields [`Poll::Ready`] or after
    /// the [`Sink`] has been completely flushed via [`Sink::poll_flush`].
    pub fn into_inner(self) -> R {
        self.inner.into_inner()
    }
}

impl<R> Stream for LengthDelimitedReader<R>
where
    R: AsyncRead,
{
    type Item = Result<Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().inner.poll_next(cx)
    }
}

impl<R> AsyncWrite for LengthDelimitedReader<R>
where
    R: AsyncWrite,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        // `this` here designates the `LengthDelimited`.
        let mut this = self.project().inner;

        // We need to flush any data previously written with the `LengthDelimited`.
        match LengthDelimited::poll_write_buffer(this.as_mut(), cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Pending => return Poll::Pending,
        }
        debug_assert!(this.write_buffer.is_empty());

        this.project().inner.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        self.project().inner.poll_close(cx)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<Result<usize, io::Error>> {
        // `this` here designates the `LengthDelimited`.
        let mut this = self.project().inner;

        // We need to flush any data previously written with the `LengthDelimited`.
        match LengthDelimited::poll_write_buffer(this.as_mut(), cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Pending => return Poll::Pending,
        }
        debug_assert!(this.write_buffer.is_empty());

        this.project().inner.poll_write_vectored(cx, bufs)
    }
}

#[cfg(test)]
mod tests {
    use crate::length_delimited::LengthDelimited;
    use async_std::net::{TcpListener, TcpStream};
    use futures::{io::Cursor, prelude::*};
    use quickcheck::*;
    use std::io::ErrorKind;

    #[test]
    fn basic_read() {
        let data = vec![6, 9, 8, 7, 6, 5, 4];
        let framed = LengthDelimited::new(Cursor::new(data));
        let recved = futures::executor::block_on(framed.try_collect::<Vec<_>>()).unwrap();
        assert_eq!(recved, vec![vec![9, 8, 7, 6, 5, 4]]);
    }

    #[test]
    fn basic_read_two() {
        let data = vec![6, 9, 8, 7, 6, 5, 4, 3, 9, 8, 7];
        let framed = LengthDelimited::new(Cursor::new(data));
        let recved = futures::executor::block_on(framed.try_collect::<Vec<_>>()).unwrap();
        assert_eq!(recved, vec![vec![9, 8, 7, 6, 5, 4], vec![9, 8, 7]]);
    }

    #[test]
    fn two_bytes_long_packet() {
        let len = 5000u16;
        assert!(len < (1 << 15));
        let frame = (0..len).map(|n| (n & 0xff) as u8).collect::<Vec<_>>();
        let mut data = vec![(len & 0x7f) as u8 | 0x80, (len >> 7) as u8];
        data.extend(frame.clone().into_iter());
        let mut framed = LengthDelimited::new(Cursor::new(data));
        let recved = futures::executor::block_on(async move { framed.next().await }).unwrap();
        assert_eq!(recved.unwrap(), frame);
    }

    #[test]
    fn packet_len_too_long() {
        let mut data = vec![0x81, 0x81, 0x1];
        data.extend((0..16513).map(|_| 0));
        let mut framed = LengthDelimited::new(Cursor::new(data));
        let recved = futures::executor::block_on(async move { framed.next().await.unwrap() });

        if let Err(io_err) = recved {
            assert_eq!(io_err.kind(), ErrorKind::InvalidData)
        } else {
            panic!()
        }
    }

    #[test]
    fn empty_frames() {
        let data = vec![0, 0, 6, 9, 8, 7, 6, 5, 4, 0, 3, 9, 8, 7];
        let framed = LengthDelimited::new(Cursor::new(data));
        let recved = futures::executor::block_on(framed.try_collect::<Vec<_>>()).unwrap();
        assert_eq!(
            recved,
            vec![
                vec![],
                vec![],
                vec![9, 8, 7, 6, 5, 4],
                vec![],
                vec![9, 8, 7],
            ]
        );
    }

    #[test]
    fn unexpected_eof_in_len() {
        let data = vec![0x89];
        let framed = LengthDelimited::new(Cursor::new(data));
        let recved = futures::executor::block_on(framed.try_collect::<Vec<_>>());
        if let Err(io_err) = recved {
            assert_eq!(io_err.kind(), ErrorKind::UnexpectedEof)
        } else {
            panic!()
        }
    }

    #[test]
    fn unexpected_eof_in_data() {
        let data = vec![5];
        let framed = LengthDelimited::new(Cursor::new(data));
        let recved = futures::executor::block_on(framed.try_collect::<Vec<_>>());
        if let Err(io_err) = recved {
            assert_eq!(io_err.kind(), ErrorKind::UnexpectedEof)
        } else {
            panic!()
        }
    }

    #[test]
    fn unexpected_eof_in_data2() {
        let data = vec![5, 9, 8, 7];
        let framed = LengthDelimited::new(Cursor::new(data));
        let recved = futures::executor::block_on(framed.try_collect::<Vec<_>>());
        if let Err(io_err) = recved {
            assert_eq!(io_err.kind(), ErrorKind::UnexpectedEof)
        } else {
            panic!()
        }
    }

    #[test]
    fn writing_reading() {
        fn prop(frames: Vec<Vec<u8>>) -> TestResult {
            async_std::task::block_on(async move {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let listener_addr = listener.local_addr().unwrap();

                let expected_frames = frames.clone();
                let server = async_std::task::spawn(async move {
                    let socket = listener.accept().await.unwrap().0;
                    let mut connec =
                        rw_stream_sink::RwStreamSink::new(LengthDelimited::new(socket));

                    let mut buf = vec![0u8; 0];
                    for expected in expected_frames {
                        if expected.is_empty() {
                            continue;
                        }
                        if buf.len() < expected.len() {
                            buf.resize(expected.len(), 0);
                        }
                        let n = connec.read(&mut buf).await.unwrap();
                        assert_eq!(&buf[..n], &expected[..]);
                    }
                });

                let client = async_std::task::spawn(async move {
                    let socket = TcpStream::connect(&listener_addr).await.unwrap();
                    let mut connec = LengthDelimited::new(socket);
                    for frame in frames {
                        connec.send(From::from(frame)).await.unwrap();
                    }
                });

                server.await;
                client.await;
            });

            TestResult::passed()
        }

        quickcheck(prop as fn(_) -> _)
    }
}
