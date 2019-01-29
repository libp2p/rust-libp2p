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

use futures::Poll;
use log::{debug, trace};
use snow;
use static_assertions::const_assert_eq;
use std::{fmt, io};
use tokio_io::{AsyncRead, AsyncWrite};

const MAX_NOISE_PKG_LEN: usize = 65535;
const MAX_WRITE_BUF_LEN: usize = 16384;

// Buffer indices ///////////////////////////////////////////////////////////

// Read buffer: [0, 65535)
const READ_BEGIN: usize = 0;
const READ_END: usize = READ_BEGIN + MAX_NOISE_PKG_LEN;

// Decryption buffer for data read: [65535, 131070)
const READ_CRYPTO_BEGIN: usize = READ_END;
const READ_CRYPTO_END: usize = READ_CRYPTO_BEGIN + MAX_NOISE_PKG_LEN;

// Write buffer: [131070, 147454)
const WRITE_BEGIN: usize = READ_CRYPTO_END;
const WRITE_END: usize = WRITE_BEGIN + MAX_WRITE_BUF_LEN;

// Encryption buffer for data to write: [147454, 180222)
const WRITE_CRYPTO_BEGIN: usize = WRITE_END;
const WRITE_CRYPTO_END: usize = WRITE_CRYPTO_BEGIN + 2 * MAX_WRITE_BUF_LEN;

const TOTAL_BUFFER_LEN: usize = WRITE_CRYPTO_END;

// Static invariants ////////////////////////////////////////////////////////

const_assert_eq!(A; READ_END - READ_BEGIN, MAX_NOISE_PKG_LEN);
const_assert_eq!(B; READ_CRYPTO_END - READ_CRYPTO_BEGIN, MAX_NOISE_PKG_LEN);
const_assert_eq!(C; WRITE_END - WRITE_BEGIN, MAX_WRITE_BUF_LEN);
const_assert_eq!(D; WRITE_CRYPTO_END - WRITE_CRYPTO_BEGIN, 2 * MAX_WRITE_BUF_LEN);
const_assert_eq!(E; 180222, TOTAL_BUFFER_LEN);
const_assert_eq!(F; 0, READ_CRYPTO_BEGIN - READ_END); // read crypto buffer follows read buffer
const_assert_eq!(G; 0, WRITE_CRYPTO_BEGIN - WRITE_END); // write crypto buffer follows write buffer

pub(super) struct Buffer {
    inner: Box<[u8; TOTAL_BUFFER_LEN]>
}

impl Buffer {
    fn borrow_mut(&mut self) -> BufferBorrow {
        let (r, w) = self.inner.split_at_mut(WRITE_BEGIN);
        let (read, read_crypto) = r.split_at_mut(READ_CRYPTO_BEGIN);
        let (write, write_crypto) = w.split_at_mut(WRITE_CRYPTO_BEGIN - WRITE_BEGIN);
        BufferBorrow { read, read_crypto, write, write_crypto }
    }
}

struct BufferBorrow<'a> {
    read: &'a mut [u8],
    read_crypto: &'a mut [u8],
    write: &'a mut [u8],
    write_crypto: &'a mut [u8]
}

pub struct NoiseOutput<T> {
    pub(super) io: T,
    pub(super) session: snow::Session,
    pub(super) buffer: Buffer,
    pub(super) read_state: ReadState,
    pub(super) write_state: WriteState
}

impl<T> fmt::Debug for NoiseOutput<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("NoiseOutput")
            .field("read_state", &self.read_state)
            .field("write_state", &self.write_state)
            .finish()
    }
}

impl<T> NoiseOutput<T> {
    pub(super) fn new(io: T, session: snow::Session) -> Self {
        NoiseOutput {
            io, session,
            buffer: Buffer { inner: Box::new([0; TOTAL_BUFFER_LEN]) },
            read_state: ReadState::Init,
            write_state: WriteState::Init
        }
    }
}

#[derive(Debug)]
pub(super) enum ReadState {
    /// initial state
    Init,
    /// read encrypted frame data
    ReadData { len: usize, off: usize },
    /// copy decrypted frame data
    CopyData { len: usize, off: usize },
    /// end of file has been reached (terminal state)
    /// The associated result signals if the EOF was unexpected or not.
    Eof(Result<(), ()>),
    /// decryption error (terminal state)
    DecErr
}

#[derive(Debug)]
pub(super) enum WriteState {
    /// initial state
    Init,
    /// accumulate write data
    BufferData { off: usize },
    /// write frame length
    WriteLen { len: usize },
    /// write out encrypted data
    WriteData { len: usize, off: usize },
    /// end of file has been reached (terminal state)
    Eof,
    /// encryption error (terminal state)
    EncErr
}

impl<T: io::Read> io::Read for NoiseOutput<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let buffer = self.buffer.borrow_mut();
        loop {
            trace!("read state: {:?}", self.read_state);
            match self.read_state {
                ReadState::Init => {
                    let n = match read_frame_len(&mut self.io)? {
                        Some(n) => n,
                        None => {
                            trace!("read: eof");
                            self.read_state = ReadState::Eof(Ok(()));
                            return Ok(0)
                        }
                    };
                    trace!("read: next frame len = {}", n);
                    if n == 0 {
                        trace!("read: empty frame");
                        continue
                    }
                    self.read_state = ReadState::ReadData { len: usize::from(n), off: 0 }
                }
                ReadState::ReadData { len, ref mut off } => {
                    let n = self.io.read(&mut buffer.read[*off .. len])?;
                    trace!("read: read {}/{} bytes", *off + n, len);
                    if n == 0 {
                        trace!("read: eof");
                        self.read_state = ReadState::Eof(Err(()));
                        return Err(io::ErrorKind::UnexpectedEof.into())
                    }
                    *off += n;
                    if len == *off {
                        trace!("read: decrypting {} bytes", len);
                        if let Ok(n) = self.session.read_message(&buffer.read[.. len], buffer.read_crypto) {
                            trace!("read: payload len = {} bytes", n);
                            self.read_state = ReadState::CopyData { len: n, off: 0 }
                        } else {
                            debug!("decryption error");
                            self.read_state = ReadState::DecErr;
                            return Err(io::ErrorKind::InvalidData.into())
                        }
                    }
                }
                ReadState::CopyData { len, ref mut off } => {
                    let n = std::cmp::min(len - *off, buf.len());
                    buf[.. n].copy_from_slice(&buffer.read_crypto[*off .. *off + n]);
                    trace!("read: copied {}/{} bytes", *off + n, len);
                    *off += n;
                    if len == *off {
                        self.read_state = ReadState::Init
                    }
                    return Ok(n)
                }
                ReadState::Eof(Ok(())) => {
                    trace!("read: eof");
                    return Ok(0)
                }
                ReadState::Eof(Err(())) => {
                    trace!("read: eof (unexpected)");
                    return Err(io::ErrorKind::UnexpectedEof.into())
                }
                ReadState::DecErr => return Err(io::ErrorKind::InvalidData.into())
            }
        }
    }
}

impl<T: io::Write> io::Write for NoiseOutput<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let buffer = self.buffer.borrow_mut();
        loop {
            trace!("write state: {:?}", self.write_state);
            match self.write_state {
                WriteState::Init => {
                    self.write_state = WriteState::BufferData { off: 0 }
                }
                WriteState::BufferData { ref mut off } => {
                    let n = std::cmp::min(MAX_WRITE_BUF_LEN - *off, buf.len());
                    buffer.write[*off .. *off + n].copy_from_slice(&buf[.. n]);
                    trace!("write: buffered {} bytes", *off + n);
                    *off += n;
                    if *off == MAX_WRITE_BUF_LEN {
                        trace!("write: encrypting {} bytes", *off);
                        if let Ok(n) = self.session.write_message(buffer.write, buffer.write_crypto) {
                            trace!("write: cipher text len = {} bytes", n);
                            self.write_state = WriteState::WriteLen { len: n }
                        } else {
                            debug!("encryption error");
                            self.write_state = WriteState::EncErr;
                            return Err(io::ErrorKind::InvalidData.into())
                        }
                    }
                    return Ok(n)
                }
                WriteState::WriteLen { len } => {
                    trace!("write: writing len ({})", len);
                    if !write_frame_len(&mut self.io, len as u16)? {
                        trace!("write: eof");
                        self.write_state = WriteState::Eof;
                        return Err(io::ErrorKind::WriteZero.into())
                    }
                    self.write_state = WriteState::WriteData { len, off: 0 }
                }
                WriteState::WriteData { len, ref mut off } => {
                    let n = self.io.write(&buffer.write_crypto[*off .. len])?;
                    trace!("write: wrote {}/{} bytes", *off + n, len);
                    if n == 0 {
                        trace!("write: eof");
                        self.write_state = WriteState::Eof;
                        return Err(io::ErrorKind::WriteZero.into())
                    }
                    *off += n;
                    if len == *off {
                        trace!("write: finished writing {} bytes", len);
                        self.write_state = WriteState::Init
                    }
                }
                WriteState::Eof => {
                    trace!("write: eof");
                    return Err(io::ErrorKind::WriteZero.into())
                }
                WriteState::EncErr => return Err(io::ErrorKind::InvalidData.into())
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        let buffer = self.buffer.borrow_mut();
        loop {
            match self.write_state {
                WriteState::Init => return Ok(()),
                WriteState::BufferData { off } => {
                    trace!("flush: encrypting {} bytes", off);
                    if let Ok(n) = self.session.write_message(&buffer.write[.. off], buffer.write_crypto) {
                        trace!("flush: cipher text len = {} bytes", n);
                        self.write_state = WriteState::WriteLen { len: n }
                    } else {
                        debug!("encryption error");
                        self.write_state = WriteState::EncErr;
                        return Err(io::ErrorKind::InvalidData.into())
                    }
                }
                WriteState::WriteLen { len } => {
                    trace!("flush: writing len ({})", len);
                    if !write_frame_len(&mut self.io, len as u16)? {
                        trace!("write: eof");
                        self.write_state = WriteState::Eof;
                        return Err(io::ErrorKind::WriteZero.into())
                    }
                    self.write_state = WriteState::WriteData { len, off: 0 }
                }
                WriteState::WriteData { len, ref mut off } => {
                    let n = self.io.write(&buffer.write_crypto[*off .. len])?;
                    trace!("flush: wrote {}/{} bytes", *off + n, len);
                    if n == 0 {
                        trace!("flush: eof");
                        self.write_state = WriteState::Eof;
                        return Err(io::ErrorKind::WriteZero.into())
                    }
                    *off += n;
                    if len == *off {
                        trace!("flush: finished writing {} bytes", len);
                        self.write_state = WriteState::Init;
                        return Ok(())
                    }
                }
                WriteState::Eof => {
                    trace!("flush: eof");
                    return Err(io::ErrorKind::WriteZero.into())
                }
                WriteState::EncErr => return Err(io::ErrorKind::InvalidData.into())
            }
        }
    }
}

impl<T: AsyncRead> AsyncRead for NoiseOutput<T> {}

impl<T: AsyncWrite> AsyncWrite for NoiseOutput<T> {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.io.shutdown()
    }
}

fn read_frame_len<R: io::Read>(io: &mut R) -> io::Result<Option<u16>> {
    let mut buf = [0, 0];
    let mut off = 0;
    loop {
        let n = io.read(&mut buf[off ..])?;
        if n == 0 {
            return Ok(None)
        }
        off += n;
        if off == 2 {
            return Ok(Some(u16::from_be_bytes(buf)))
        }
    }
}

fn write_frame_len<W: io::Write>(io: &mut W, len: u16) -> io::Result<bool> {
    let buf = len.to_be_bytes();
    let mut off = 0;
    loop {
        let n = io.write(&buf[off ..])?;
        if n == 0 {
            return Ok(false)
        }
        off += n;
        if off == 2 {
            return Ok(true)
        }
    }
}

