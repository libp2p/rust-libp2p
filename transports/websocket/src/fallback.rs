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
use std::io::{self, Read};
use futures::{AsyncRead, AsyncWrite};
use bytes5::Buf;

use crate::framed::TlsOrPlain;

#[derive(Debug)]
pub struct Fallback<T> {
	pub(crate) stream: TlsOrPlain<T>,
	pub(crate) bytes: bytes5::Bytes
}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncRead for Fallback<T> {
	fn poll_read(self: Pin<&mut Self>, cx: &mut Context, buf: &mut [u8]) -> Poll<io::Result<usize>> {
		let this = Pin::into_inner(self);

		if !this.bytes.is_empty() {
			match (&*this.bytes).read(buf) {
				Ok(read) => {
					this.bytes.advance(read);
					Poll::Ready(Ok(read))
				}
				Err(err) => Poll::Ready(Err(err))
			}
		} else {
			Pin::new(&mut this.stream).poll_read(cx, buf)
		}
	}
}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncWrite for Fallback<T> {
	fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize, io::Error>> {
		Pin::new(&mut Pin::into_inner(self).stream).poll_write(cx, buf)
	}

	fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
		Pin::new(&mut Pin::into_inner(self).stream).poll_flush(cx)
	}

	fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
		Pin::new(&mut Pin::into_inner(self).stream).poll_close(cx)
	}
}
