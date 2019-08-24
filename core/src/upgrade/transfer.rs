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

//! Contains some helper futures for creating upgrades.

use futures::{prelude::*, try_ready};
use std::{cmp, error, fmt, io::Cursor, mem};
use tokio_io::{io, AsyncRead, AsyncWrite};

/// Send a message to the given socket, then shuts down the writing side.
///
/// > **Note**: Prepends a variable-length prefix indicate the length of the message. This is
/// >           compatible with what `read_one` expects.
pub fn write_one<TSocket, TData>(socket: TSocket, data: TData) -> WriteOne<TSocket, TData>
where
    TSocket: AsyncWrite,
    TData: AsRef<[u8]>,
{
    let len_data = build_int_buffer(data.as_ref().len());
    WriteOne {
        inner: WriteOneInner::WriteLen(io::write_all(socket, len_data), data),
    }
}

/// Builds a buffer that contains the given integer encoded as variable-length.
fn build_int_buffer(num: usize) -> io::Window<[u8; 10]> {
    let mut len_data = unsigned_varint::encode::u64_buffer();
    let encoded_len = unsigned_varint::encode::u64(num as u64, &mut len_data).len();
    let mut len_data = io::Window::new(len_data);
    len_data.set_end(encoded_len);
    len_data
}

/// Future that makes `write_one` work.
#[derive(Debug)]
pub struct WriteOne<TSocket, TData = Vec<u8>> {
    inner: WriteOneInner<TSocket, TData>,
}

#[derive(Debug)]
enum WriteOneInner<TSocket, TData> {
    /// We need to write the data length to the socket.
    WriteLen(io::WriteAll<TSocket, io::Window<[u8; 10]>>, TData),
    /// We need to write the actual data to the socket.
    Write(io::WriteAll<TSocket, TData>),
    /// We need to shut down the socket.
    Shutdown(io::Shutdown<TSocket>),
    /// A problem happened during the processing.
    Poisoned,
}

impl<TSocket, TData> Future for WriteOne<TSocket, TData>
where
    TSocket: AsyncWrite,
    TData: AsRef<[u8]>,
{
    type Item = ();
    type Error = std::io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        Ok(self.inner.poll()?.map(|_socket| ()))
    }
}

impl<TSocket, TData> Future for WriteOneInner<TSocket, TData>
where
    TSocket: AsyncWrite,
    TData: AsRef<[u8]>,
{
    type Item = TSocket;
    type Error = std::io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(self, WriteOneInner::Poisoned) {
                WriteOneInner::WriteLen(mut inner, data) => match inner.poll()? {
                    Async::Ready((socket, _)) => {
                        *self = WriteOneInner::Write(io::write_all(socket, data));
                    }
                    Async::NotReady => {
                        *self = WriteOneInner::WriteLen(inner, data);
                    }
                },
                WriteOneInner::Write(mut inner) => match inner.poll()? {
                    Async::Ready((socket, _)) => {
                        *self = WriteOneInner::Shutdown(tokio_io::io::shutdown(socket));
                    }
                    Async::NotReady => {
                        *self = WriteOneInner::Write(inner);
                    }
                },
                WriteOneInner::Shutdown(ref mut inner) => {
                    let socket = try_ready!(inner.poll());
                    return Ok(Async::Ready(socket));
                }
                WriteOneInner::Poisoned => panic!(),
            }
        }
    }
}

/// Reads a message from the given socket. Only one message is processed and the socket is dropped,
/// because we assume that the socket will not send anything more.
///
/// The `max_size` parameter is the maximum size in bytes of the message that we accept. This is
/// necessary in order to avoid DoS attacks where the remote sends us a message of several
/// gigabytes.
///
/// > **Note**: Assumes that a variable-length prefix indicates the length of the message. This is
/// >           compatible with what `write_one` does.
pub fn read_one<TSocket>(
    socket: TSocket,
    max_size: usize,
) -> ReadOne<TSocket>
{
    ReadOne {
        inner: ReadOneInner::ReadLen {
            socket,
            len_buf: Cursor::new([0; 10]),
            max_size,
        },
    }
}

/// Future that makes `read_one` work.
#[derive(Debug)]
pub struct ReadOne<TSocket> {
    inner: ReadOneInner<TSocket>,
}

#[derive(Debug)]
enum ReadOneInner<TSocket> {
    // We need to read the data length from the socket.
    ReadLen {
        socket: TSocket,
        /// A small buffer where we will right the variable-length integer representing the
        /// length of the actual packet.
        len_buf: Cursor<[u8; 10]>,
        max_size: usize,
    },
    // We need to read the actual data from the socket.
    ReadRest(io::ReadExact<TSocket, io::Window<Vec<u8>>>),
    /// A problem happened during the processing.
    Poisoned,
}

impl<TSocket> Future for ReadOne<TSocket>
where
    TSocket: AsyncRead,
{
    type Item = Vec<u8>;
    type Error = ReadOneError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        Ok(self.inner.poll()?.map(|(_, out)| out))
    }
}

impl<TSocket> Future for ReadOneInner<TSocket>
where
    TSocket: AsyncRead,
{
    type Item = (TSocket, Vec<u8>);
    type Error = ReadOneError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(self, ReadOneInner::Poisoned) {
                ReadOneInner::ReadLen {
                    mut socket,
                    mut len_buf,
                    max_size,
                } => {
                    match socket.read_buf(&mut len_buf)? {
                        Async::Ready(num_read) => {
                            // Reaching EOF before finishing to read the length is an error, unless
                            // the EOF is at the very beginning of the substream, in which case we
                            // assume that the data is empty.
                            if num_read == 0 {
                                if len_buf.position() == 0 {
                                    return Ok(Async::Ready((socket, Vec::new())));
                                } else {
                                    return Err(ReadOneError::Io(
                                        std::io::ErrorKind::UnexpectedEof.into(),
                                    ));
                                }
                            }

                            let len_buf_with_data =
                                &len_buf.get_ref()[..len_buf.position() as usize];
                            if let Ok((len, data_start)) =
                                unsigned_varint::decode::usize(len_buf_with_data)
                            {
                                if len >= max_size {
                                    return Err(ReadOneError::TooLarge {
                                        requested: len,
                                        max: max_size,
                                    });
                                }

                                // Create `data_buf` containing the start of the data that was
                                // already in `len_buf`.
                                let n = cmp::min(data_start.len(), len);
                                let mut data_buf = vec![0; len];
                                data_buf[.. n].copy_from_slice(&data_start[.. n]);
                                let mut data_buf = io::Window::new(data_buf);
                                data_buf.set_start(data_start.len());
                                *self = ReadOneInner::ReadRest(io::read_exact(socket, data_buf));
                            } else {
                                *self = ReadOneInner::ReadLen {
                                    socket,
                                    len_buf,
                                    max_size,
                                };
                            }
                        }
                        Async::NotReady => {
                            *self = ReadOneInner::ReadLen {
                                socket,
                                len_buf,
                                max_size,
                            };
                            return Ok(Async::NotReady);
                        }
                    }
                }
                ReadOneInner::ReadRest(mut inner) => {
                    match inner.poll()? {
                        Async::Ready((socket, data)) => {
                            return Ok(Async::Ready((socket, data.into_inner())));
                        }
                        Async::NotReady => {
                            *self = ReadOneInner::ReadRest(inner);
                            return Ok(Async::NotReady);
                        }
                    }
                }
                ReadOneInner::Poisoned => panic!(),
            }
        }
    }
}

/// Error while reading one message.
#[derive(Debug)]
pub enum ReadOneError {
    /// Error on the socket.
    Io(std::io::Error),
    /// Requested data is over the maximum allowed size.
    TooLarge {
        /// Size requested by the remote.
        requested: usize,
        /// Maximum allowed.
        max: usize,
    },
}

impl From<std::io::Error> for ReadOneError {
    fn from(err: std::io::Error) -> ReadOneError {
        ReadOneError::Io(err)
    }
}

impl fmt::Display for ReadOneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            ReadOneError::Io(ref err) => write!(f, "{}", err),
            ReadOneError::TooLarge { .. } => write!(f, "Received data size over maximum"),
        }
    }
}

impl error::Error for ReadOneError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match *self {
            ReadOneError::Io(ref err) => Some(err),
            ReadOneError::TooLarge { .. } => None,
        }
    }
}

/// Similar to `read_one`, but applies a transformation on the output buffer.
///
/// > **Note**: The `param` parameter is an arbitrary value that will be passed back to `then`.
/// >           This parameter is normally not necessary, as we could just pass a closure that has
/// >           ownership of any data we want. In practice, though, this would make the
/// >           `ReadRespond` type impossible to express as a concrete type. Once the `impl Trait`
/// >           syntax is allowed within traits, we can remove this parameter.
pub fn read_one_then<TSocket, TParam, TThen, TOut, TErr>(
    socket: TSocket,
    max_size: usize,
    param: TParam,
    then: TThen,
) -> ReadOneThen<TSocket, TParam, TThen>
where
    TSocket: AsyncRead,
    TThen: FnOnce(Vec<u8>, TParam) -> Result<TOut, TErr>,
    TErr: From<ReadOneError>,
{
    ReadOneThen {
        inner: read_one(socket, max_size),
        then: Some((param, then)),
    }
}

/// Future that makes `read_one_then` work.
#[derive(Debug)]
pub struct ReadOneThen<TSocket, TParam, TThen> {
    inner: ReadOne<TSocket>,
    then: Option<(TParam, TThen)>,
}

impl<TSocket, TParam, TThen, TOut, TErr> Future for ReadOneThen<TSocket, TParam, TThen>
where
    TSocket: AsyncRead,
    TThen: FnOnce(Vec<u8>, TParam) -> Result<TOut, TErr>,
    TErr: From<ReadOneError>,
{
    type Item = TOut;
    type Error = TErr;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll()? {
            Async::Ready(buffer) => {
                let (param, then) = self.then.take()
                    .expect("Future was polled after it was finished");
                Ok(Async::Ready(then(buffer, param)?))
            },
            Async::NotReady => Ok(Async::NotReady),
        }
    }
}

/// Similar to `read_one`, but applies a transformation on the output buffer.
///
/// > **Note**: The `param` parameter is an arbitrary value that will be passed back to `then`.
/// >           This parameter is normally not necessary, as we could just pass a closure that has
/// >           ownership of any data we want. In practice, though, this would make the
/// >           `ReadRespond` type impossible to express as a concrete type. Once the `impl Trait`
/// >           syntax is allowed within traits, we can remove this parameter.
pub fn read_respond<TSocket, TThen, TParam, TOut, TErr>(
    socket: TSocket,
    max_size: usize,
    param: TParam,
    then: TThen,
) -> ReadRespond<TSocket, TParam, TThen>
where
    TSocket: AsyncRead,
    TThen: FnOnce(TSocket, Vec<u8>, TParam) -> Result<TOut, TErr>,
    TErr: From<ReadOneError>,
{
    ReadRespond {
        inner: read_one(socket, max_size).inner,
        then: Some((then, param)),
    }
}

/// Future that makes `read_respond` work.
#[derive(Debug)]
pub struct ReadRespond<TSocket, TParam, TThen> {
    inner: ReadOneInner<TSocket>,
    then: Option<(TThen, TParam)>,
}

impl<TSocket, TThen, TParam, TOut, TErr> Future for ReadRespond<TSocket, TParam, TThen>
where
    TSocket: AsyncRead,
    TThen: FnOnce(TSocket, Vec<u8>, TParam) -> Result<TOut, TErr>,
    TErr: From<ReadOneError>,
{
    type Item = TOut;
    type Error = TErr;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll()? {
            Async::Ready((socket, buffer)) => {
                let (then, param) = self.then.take().expect("Future was polled after it was finished");
                Ok(Async::Ready(then(socket, buffer, param)?))
            },
            Async::NotReady => Ok(Async::NotReady),
        }
    }
}

/// Send a message to the given socket, then shuts down the writing side, then reads an answer.
///
/// This combines `write_one` followed with `read_one_then`.
///
/// > **Note**: The `param` parameter is an arbitrary value that will be passed back to `then`.
/// >           This parameter is normally not necessary, as we could just pass a closure that has
/// >           ownership of any data we want. In practice, though, this would make the
/// >           `ReadRespond` type impossible to express as a concrete type. Once the `impl Trait`
/// >           syntax is allowed within traits, we can remove this parameter.
pub fn request_response<TSocket, TData, TParam, TThen, TOut, TErr>(
    socket: TSocket,
    data: TData,
    max_size: usize,
    param: TParam,
    then: TThen,
) -> RequestResponse<TSocket, TParam, TThen, TData>
where
    TSocket: AsyncRead + AsyncWrite,
    TData: AsRef<[u8]>,
    TThen: FnOnce(Vec<u8>, TParam) -> Result<TOut, TErr>,
{
    RequestResponse {
        inner: RequestResponseInner::Write(write_one(socket, data).inner, max_size, param, then),
    }
}

/// Future that makes `request_response` work.
#[derive(Debug)]
pub struct RequestResponse<TSocket, TParam, TThen, TData = Vec<u8>> {
    inner: RequestResponseInner<TSocket, TData, TParam, TThen>,
}

#[derive(Debug)]
enum RequestResponseInner<TSocket, TData, TParam, TThen> {
    // We need to write data to the socket.
    Write(WriteOneInner<TSocket, TData>, usize, TParam, TThen),
    // We need to read the message.
    Read(ReadOneThen<TSocket, TParam, TThen>),
    // An error happened during the processing.
    Poisoned,
}

impl<TSocket, TData, TParam, TThen, TOut, TErr> Future for RequestResponse<TSocket, TParam, TThen, TData>
where
    TSocket: AsyncRead + AsyncWrite,
    TData: AsRef<[u8]>,
    TThen: FnOnce(Vec<u8>, TParam) -> Result<TOut, TErr>,
    TErr: From<ReadOneError>,
{
    type Item = TOut;
    type Error = TErr;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.inner, RequestResponseInner::Poisoned) {
                RequestResponseInner::Write(mut inner, max_size, param, then) => {
                    match inner.poll().map_err(ReadOneError::Io)? {
                        Async::Ready(socket) => {
                            self.inner =
                                RequestResponseInner::Read(read_one_then(socket, max_size, param, then));
                        }
                        Async::NotReady => {
                            self.inner = RequestResponseInner::Write(inner, max_size, param, then);
                            return Ok(Async::NotReady);
                        }
                    }
                }
                RequestResponseInner::Read(mut inner) => match inner.poll()? {
                    Async::Ready(packet) => return Ok(Async::Ready(packet)),
                    Async::NotReady => {
                        self.inner = RequestResponseInner::Read(inner);
                        return Ok(Async::NotReady);
                    }
                },
                RequestResponseInner::Poisoned => panic!(),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor};
    use tokio::runtime::current_thread::Runtime;

    #[test]
    fn write_one_works() {
        let data = (0..rand::random::<usize>() % 10_000)
            .map(|_| rand::random::<u8>())
            .collect::<Vec<_>>();

        let mut out = vec![0; 10_000];
        let future = write_one(Cursor::new(&mut out[..]), data.clone());
        Runtime::new().unwrap().block_on(future).unwrap();

        let (out_len, out_data) = unsigned_varint::decode::usize(&out).unwrap();
        assert_eq!(out_len, data.len());
        assert_eq!(&out_data[..out_len], &data[..]);
    }

    #[test]
    fn read_one_works() {
        let original_data = (0..rand::random::<usize>() % 10_000)
            .map(|_| rand::random::<u8>())
            .collect::<Vec<_>>();

        let mut len_buf = unsigned_varint::encode::usize_buffer();
        let len_buf = unsigned_varint::encode::usize(original_data.len(), &mut len_buf);

        let mut in_buffer = len_buf.to_vec();
        in_buffer.extend_from_slice(&original_data);

        let future = read_one_then(Cursor::new(in_buffer), 10_000, (), move |out, ()| -> Result<_, ReadOneError> {
            assert_eq!(out, original_data);
            Ok(())
        });

        Runtime::new().unwrap().block_on(future).unwrap();
    }

    #[test]
    fn read_one_zero_len() {
        let future = read_one_then(Cursor::new(vec![0]), 10_000, (), move |out, ()| -> Result<_, ReadOneError> {
            assert!(out.is_empty());
            Ok(())
        });

        Runtime::new().unwrap().block_on(future).unwrap();
    }

    #[test]
    fn read_checks_length() {
        let mut len_buf = unsigned_varint::encode::u64_buffer();
        let len_buf = unsigned_varint::encode::u64(5_000, &mut len_buf);

        let mut in_buffer = len_buf.to_vec();
        in_buffer.extend((0..5000).map(|_| 0));

        let future = read_one_then(Cursor::new(in_buffer), 100, (), move |_, ()| -> Result<_, ReadOneError> {
            Ok(())
        });

        match Runtime::new().unwrap().block_on(future) {
            Err(ReadOneError::TooLarge { .. }) => (),
            _ => panic!(),
        }
    }

    #[test]
    fn read_one_accepts_empty() {
        let future = read_one_then(Cursor::new([]), 10_000, (), move |out, ()| -> Result<_, ReadOneError> {
            assert!(out.is_empty());
            Ok(())
        });

        Runtime::new().unwrap().block_on(future).unwrap();
    }

    #[test]
    fn read_one_eof_before_len() {
        let future = read_one_then(Cursor::new([0x80]), 10_000, (), move |_, ()| -> Result<(), ReadOneError> {
            unreachable!()
        });

        match Runtime::new().unwrap().block_on(future) {
            Err(ReadOneError::Io(ref err)) if err.kind() == io::ErrorKind::UnexpectedEof => (),
            _ => panic!()
        }
    }
}
