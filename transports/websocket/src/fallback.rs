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

use std::pin::Pin;
use std::task::{Poll, Context};
use std::io::{self, Read, IoSliceMut};
use futures::{AsyncRead, AsyncWrite};
use bytes5::Buf;

use crate::framed::TlsOrPlain;

#[derive(Debug)]
pub struct Fallback<T> {
	pub(crate) stream: TlsOrPlain<T>,
	pub(crate) bytes: bytes5::Bytes
}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncRead for Fallback<T> {
	fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context, buf: &mut [u8]) -> Poll<io::Result<usize>> {
		if !self.bytes.is_empty() {
			match (&*self.bytes).read(buf) {
				Ok(read) => {
					self.bytes.advance(read);
					Poll::Ready(Ok(read))
				}
				Err(err) => Poll::Ready(Err(err))
			}
		} else {
			Pin::new(&mut self.stream).poll_read(cx, buf)
		}
	}

	fn poll_read_vectored(mut self: Pin<&mut Self>, cx: &mut Context, bufs: &mut [IoSliceMut]) -> Poll<io::Result<usize>> {
		if !self.bytes.is_empty() {
			match (&*self.bytes).read_vectored(bufs) {
				Ok(read) => {
					self.bytes.advance(read);
					Poll::Ready(Ok(read))
				}
				Err(err) => Poll::Ready(Err(err))
			}
		} else {
			Pin::new(&mut self.stream).poll_read_vectored(cx, bufs)
		}
	}
}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncWrite for Fallback<T> {
	fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize, io::Error>> {
		Pin::new(&mut self.stream).poll_write(cx, buf)
	}

	fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
		Pin::new(&mut self.stream).poll_flush(cx)
	}

	fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
		Pin::new(&mut self.stream).poll_close(cx)
	}
}
