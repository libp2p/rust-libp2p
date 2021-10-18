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

use futures::prelude::*;
use std::io;

// TODO: these methods could be on an Ext trait to AsyncWrite

/// Writes a message to the given socket with a length prefix appended to it. Also flushes the socket.
///
/// > **Note**: Prepends a variable-length prefix indicate the length of the message. This is
/// >           compatible with what [`read_length_prefixed`] expects.
pub async fn write_length_prefixed(
    socket: &mut (impl AsyncWrite + Unpin),
    data: impl AsRef<[u8]>,
) -> Result<(), io::Error> {
    write_varint(socket, data.as_ref().len()).await?;
    socket.write_all(data.as_ref()).await?;
    socket.flush().await?;

    Ok(())
}

/// Writes a variable-length integer to the `socket`.
///
/// > **Note**: Does **NOT** flush the socket.
pub async fn write_varint(
    socket: &mut (impl AsyncWrite + Unpin),
    len: usize,
) -> Result<(), io::Error> {
    let mut len_data = unsigned_varint::encode::usize_buffer();
    let encoded_len = unsigned_varint::encode::usize(len, &mut len_data).len();
    socket.write_all(&len_data[..encoded_len]).await?;

    Ok(())
}

/// Reads a variable-length integer from the `socket`.
///
/// As a special exception, if the `socket` is empty and EOFs right at the beginning, then we
/// return `Ok(0)`.
///
/// > **Note**: This function reads bytes one by one from the `socket`. It is therefore encouraged
/// >           to use some sort of buffering mechanism.
pub async fn read_varint(socket: &mut (impl AsyncRead + Unpin)) -> Result<usize, io::Error> {
    let mut buffer = unsigned_varint::encode::usize_buffer();
    let mut buffer_len = 0;

    loop {
        match socket.read(&mut buffer[buffer_len..buffer_len + 1]).await? {
            0 => {
                // Reaching EOF before finishing to read the length is an error, unless the EOF is
                // at the very beginning of the substream, in which case we assume that the data is
                // empty.
                if buffer_len == 0 {
                    return Ok(0);
                } else {
                    return Err(io::ErrorKind::UnexpectedEof.into());
                }
            }
            n => debug_assert_eq!(n, 1),
        }

        buffer_len += 1;

        match unsigned_varint::decode::usize(&buffer[..buffer_len]) {
            Ok((len, _)) => return Ok(len),
            Err(unsigned_varint::decode::Error::Overflow) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "overflow in variable-length integer",
                ));
            }
            // TODO: why do we have a `__Nonexhaustive` variant in the error? I don't know how to process it
            // Err(unsigned_varint::decode::Error::Insufficient) => {}
            Err(_) => {}
        }
    }
}

/// Reads a length-prefixed message from the given socket.
///
/// The `max_size` parameter is the maximum size in bytes of the message that we accept. This is
/// necessary in order to avoid DoS attacks where the remote sends us a message of several
/// gigabytes.
///
/// > **Note**: Assumes that a variable-length prefix indicates the length of the message. This is
/// >           compatible with what [`write_length_prefixed`] does.
pub async fn read_length_prefixed(
    socket: &mut (impl AsyncRead + Unpin),
    max_size: usize,
) -> io::Result<Vec<u8>> {
    let len = read_varint(socket).await?;
    if len > max_size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Received data size ({} bytes) exceeds maximum ({} bytes)",
                len, max_size
            ),
        ));
    }

    let mut buf = vec![0; len];
    socket.read_exact(&mut buf).await?;

    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_length_prefixed_works() {
        let data = (0..rand::random::<usize>() % 10_000)
            .map(|_| rand::random::<u8>())
            .collect::<Vec<_>>();
        let mut out = vec![0; 10_000];

        futures::executor::block_on(async {
            let mut socket = futures::io::Cursor::new(&mut out[..]);

            write_length_prefixed(&mut socket, &data).await.unwrap();
            socket.close().await.unwrap();
        });

        let (out_len, out_data) = unsigned_varint::decode::usize(&out).unwrap();
        assert_eq!(out_len, data.len());
        assert_eq!(&out_data[..out_len], &data[..]);
    }

    // TODO: rewrite these tests
    /*
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

        futures::executor::block_on(future).unwrap();
    }

    #[test]
    fn read_one_zero_len() {
        let future = read_one_then(Cursor::new(vec![0]), 10_000, (), move |out, ()| -> Result<_, ReadOneError> {
            assert!(out.is_empty());
            Ok(())
        });

        futures::executor::block_on(future).unwrap();
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

        match futures::executor::block_on(future) {
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

        futures::executor::block_on(future).unwrap();
    }

    #[test]
    fn read_one_eof_before_len() {
        let future = read_one_then(Cursor::new([0x80]), 10_000, (), move |_, ()| -> Result<(), ReadOneError> {
            unreachable!()
        });

        match futures::executor::block_on(future) {
            Err(ReadOneError::Io(ref err)) if err.kind() == io::ErrorKind::UnexpectedEof => (),
            _ => panic!()
        }
    }*/
}
