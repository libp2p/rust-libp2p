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

use bytes::{Bytes, BytesMut, BufMut};
use futures::{try_ready, Async, Poll, Sink, StartSend, Stream, AsyncSink};
use std::{io, u16};
use tokio_io::{AsyncRead, AsyncWrite};
use unsigned_varint as uvi;

const MAX_LEN_BYTES: u16 = 2;
const MAX_FRAME_SIZE: u16 = (1 << (MAX_LEN_BYTES * 8 - MAX_LEN_BYTES)) - 1;
const DEFAULT_BUFFER_SIZE: usize = 64;

/// `Stream` and `Sink` wrapping some `AsyncRead + AsyncWrite` resource to read
/// and write unsigned-varint prefixed frames.
///
/// We purposely only support a frame sizes up to 16KiB (2 bytes unsigned varint
/// frame length). Frames mostly consist in a short protocol name, which is highly
/// unlikely to be more than 16KiB long.
pub struct LengthDelimited<R> {
    /// The inner I/O resource.
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
    ReadLength { buf: [u8; MAX_LEN_BYTES as usize], pos: usize },
    /// We are currently reading the frame of data itself.
    ReadData { len: u16, pos: usize },
}

impl Default for ReadState {
    fn default() -> Self {
        ReadState::ReadLength {
            buf: [0; MAX_LEN_BYTES as usize],
            pos: 0
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

    /// Destroys the `LengthDelimited` and returns the underlying socket.
    ///
    /// This method is guaranteed not to skip any data from the socket.
    ///
    /// # Panic
    ///
    /// Will panic if called while there is data inside the read or write buffer.
    /// **This can only happen if you call `poll()` manually**. Using this struct
    /// as it is intended to be used (i.e. through the high-level `futures` API)
    /// will always leave the object in a state in which `into_inner()` will not panic.
    pub fn into_inner(self) -> R {
        assert!(self.write_buffer.is_empty());
        assert!(self.read_buffer.is_empty());
        self.inner
    }
}

impl<R> Stream for LengthDelimited<R>
where
    R: AsyncRead
{
    type Item = Bytes;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            match &mut self.read_state {
                ReadState::ReadLength { buf, pos } => {
                    match self.inner.read(&mut buf[*pos .. *pos + 1]) {
                        Ok(0) => {
                            if *pos == 0 {
                                return Ok(Async::Ready(None));
                            } else {
                                return Err(io::ErrorKind::UnexpectedEof.into());
                            }
                        }
                        Ok(n) => {
                            debug_assert_eq!(n, 1);
                            *pos += n;
                        }
                        Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                            return Ok(Async::NotReady);
                        }
                        Err(err) => {
                            return Err(err);
                        }
                    };

                    if (buf[*pos - 1] & 0x80) == 0 {
                        // MSB is not set, indicating the end of the length prefix.
                        let (len, _) = uvi::decode::u16(buf).map_err(|e| {
                            log::debug!("invalid length prefix: {}", e);
                            io::Error::new(io::ErrorKind::InvalidData, "invalid length prefix")
                        })?;

                        if len >= 1 {
                            self.read_state = ReadState::ReadData { len, pos: 0 };
                            self.read_buffer.resize(len as usize, 0);
                        } else {
                            debug_assert_eq!(len, 0);
                            self.read_state = ReadState::default();
                            return Ok(Async::Ready(Some(Bytes::new())));
                        }
                    } else if *pos == MAX_LEN_BYTES as usize {
                        // MSB signals more length bytes but we have already read the maximum.
                        // See the module documentation about the max frame len.
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Maximum frame length exceeded"));
                    }
                }
                ReadState::ReadData { len, pos } => {
                    match self.inner.read(&mut self.read_buffer[*pos..]) {
                        Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                        Ok(n) => *pos += n,
                        Err(err) =>
                            if err.kind() == io::ErrorKind::WouldBlock {
                                return Ok(Async::NotReady)
                            } else {
                                return Err(err)
                            }
                    };
                    if *pos == *len as usize {
                        // Finished reading the frame.
                        let frame = self.read_buffer.split_off(0).freeze();
                        self.read_state = ReadState::default();
                        return Ok(Async::Ready(Some(frame)));
                    }
                }
            }
        }
    }
}

impl<R> Sink for LengthDelimited<R>
where
    R: AsyncWrite,
{
    type SinkItem = Bytes;
    type SinkError = io::Error;

    fn start_send(&mut self, msg: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        // Use the maximum frame length also as a (soft) upper limit
        // for the entire write buffer. The actual (hard) limit is thus
        // implied to be roughly 2 * MAX_FRAME_SIZE.
        if self.write_buffer.len() >= MAX_FRAME_SIZE as usize {
            self.poll_complete()?;
            if self.write_buffer.len() >= MAX_FRAME_SIZE as usize {
                return Ok(AsyncSink::NotReady(msg))
            }
        }

        let len = msg.len() as u16;
        if len > MAX_FRAME_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Maximum frame size exceeded."))
        }

        let mut uvi_buf = uvi::encode::u16_buffer();
        let uvi_len = uvi::encode::u16(len, &mut uvi_buf);
        self.write_buffer.reserve(len as usize + uvi_len.len());
        self.write_buffer.put(uvi_len);
        self.write_buffer.put(msg);

        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        while !self.write_buffer.is_empty() {
            let n = try_ready!(self.inner.poll_write(&self.write_buffer));

            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "Failed to write buffered frame."))
            }

            let _ = self.write_buffer.split_to(n);
        }

        try_ready!(self.inner.poll_flush());
        return Ok(Async::Ready(()));
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        try_ready!(self.poll_complete());
        Ok(self.inner.shutdown()?)
    }
}

#[cfg(test)]
mod tests {
    use futures::{Future, Stream};
    use crate::length_delimited::LengthDelimited;
    use std::io::{Cursor, ErrorKind};

    #[test]
    fn basic_read() {
        let data = vec![6, 9, 8, 7, 6, 5, 4];
        let framed = LengthDelimited::new(Cursor::new(data));
        let recved = framed.collect().wait().unwrap();
        assert_eq!(recved, vec![vec![9, 8, 7, 6, 5, 4]]);
    }

    #[test]
    fn basic_read_two() {
        let data = vec![6, 9, 8, 7, 6, 5, 4, 3, 9, 8, 7];
        let framed = LengthDelimited::new(Cursor::new(data));
        let recved = framed.collect().wait().unwrap();
        assert_eq!(recved, vec![vec![9, 8, 7, 6, 5, 4], vec![9, 8, 7]]);
    }

    #[test]
    fn two_bytes_long_packet() {
        let len = 5000u16;
        assert!(len < (1 << 15));
        let frame = (0..len).map(|n| (n & 0xff) as u8).collect::<Vec<_>>();
        let mut data = vec![(len & 0x7f) as u8 | 0x80, (len >> 7) as u8];
        data.extend(frame.clone().into_iter());
        let framed = LengthDelimited::new(Cursor::new(data));
        let recved = framed
            .into_future()
            .map(|(m, _)| m)
            .map_err(|_| ())
            .wait()
            .unwrap();
        assert_eq!(recved.unwrap(), frame);
    }

    #[test]
    fn packet_len_too_long() {
        let mut data = vec![0x81, 0x81, 0x1];
        data.extend((0..16513).map(|_| 0));
        let framed = LengthDelimited::new(Cursor::new(data));
        let recved = framed
            .into_future()
            .map(|(m, _)| m)
            .map_err(|(err, _)| err)
            .wait();

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
        let recved = framed.collect().wait().unwrap();
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
        let recved = framed.collect().wait();
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
        let recved = framed.collect().wait();
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
        let recved = framed.collect().wait();
        if let Err(io_err) = recved {
            assert_eq!(io_err.kind(), ErrorKind::UnexpectedEof)
        } else {
            panic!()
        }
    }
}

