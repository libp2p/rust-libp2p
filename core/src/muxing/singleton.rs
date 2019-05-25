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

use crate::{Endpoint, muxing::StreamMuxer};
use futures::prelude::*;
use parking_lot::Mutex;
use std::{io, sync::atomic::{AtomicBool, Ordering}};
use tokio_io::{AsyncRead, AsyncWrite};

/// Implementation of `StreamMuxer` that allows only one substream on top of a connection,
/// yielding the connection itself.
///
/// Applying this muxer on a connection doesn't read or write any data on the connection itself.
/// Most notably, no protocol is negotiated.
pub struct SingletonMuxer<TSocket> {
    /// The inner connection.
    inner: Mutex<TSocket>,
    /// If true, a substream has been produced and any further attempt should fail.
    substream_extracted: AtomicBool,
    /// Our local endpoint. Always the same value as was passed to `new`.
    endpoint: Endpoint,
    /// If true, we have received data from the remote.
    remote_acknowledged: AtomicBool,
}

impl<TSocket> SingletonMuxer<TSocket> {
    /// Creates a new `SingletonMuxer`.
    ///
    /// If `endpoint` is `Dialer`, then only one outbound substream will be permitted.
    /// If `endpoint` is `Listener`, then only one inbound substream will be permitted.
    pub fn new(inner: TSocket, endpoint: Endpoint) -> Self {
        SingletonMuxer {
            inner: Mutex::new(inner),
            substream_extracted: AtomicBool::new(false),
            endpoint,
            remote_acknowledged: AtomicBool::new(false),
        }
    }
}

/// Substream of the `SingletonMuxer`.
pub struct Substream {}
/// Outbound substream attempt of the `SingletonMuxer`.
pub struct OutboundSubstream {}

impl<TSocket> StreamMuxer for SingletonMuxer<TSocket>
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Substream = Substream;
    type OutboundSubstream = OutboundSubstream;
    type Error = io::Error;

    fn poll_inbound(&self) -> Poll<Self::Substream, io::Error> {
        match self.endpoint {
            Endpoint::Dialer => return Ok(Async::NotReady),
            Endpoint::Listener => {}
        }

        if !self.substream_extracted.swap(true, Ordering::Relaxed) {
            Ok(Async::Ready(Substream {}))
        } else {
            Ok(Async::NotReady)
        }
    }

    fn open_outbound(&self) -> Self::OutboundSubstream {
        OutboundSubstream {}
    }

    fn poll_outbound(&self, _: &mut Self::OutboundSubstream) -> Poll<Self::Substream, io::Error> {
        match self.endpoint {
            Endpoint::Listener => return Ok(Async::NotReady),
            Endpoint::Dialer => {}
        }

        if !self.substream_extracted.swap(true, Ordering::Relaxed) {
            Ok(Async::Ready(Substream {}))
        } else {
            Ok(Async::NotReady)
        }
    }

    fn destroy_outbound(&self, _: Self::OutboundSubstream) {
    }

    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.inner.lock().prepare_uninitialized_buffer(buf)
    }

    fn read_substream(&self, _: &mut Self::Substream, buf: &mut [u8]) -> Poll<usize, io::Error> {
        let res = self.inner.lock().poll_read(buf);
        if let Ok(Async::Ready(_)) = res {
            self.remote_acknowledged.store(true, Ordering::Release);
        }
        res
    }

    fn write_substream(&self, _: &mut Self::Substream, buf: &[u8]) -> Poll<usize, io::Error> {
        self.inner.lock().poll_write(buf)
    }

    fn flush_substream(&self, _: &mut Self::Substream) -> Poll<(), io::Error> {
        self.inner.lock().poll_flush()
    }

    fn shutdown_substream(&self, _: &mut Self::Substream) -> Poll<(), io::Error> {
        self.inner.lock().shutdown()
    }

    fn destroy_substream(&self, _: Self::Substream) {
    }

    fn is_remote_acknowledged(&self) -> bool {
        self.remote_acknowledged.load(Ordering::Acquire)
    }

    fn close(&self) -> Poll<(), io::Error> {
        // The `StreamMuxer` trait requires that `close()` implies `flush_all()`.
        self.flush_all()
    }

    fn flush_all(&self) -> Poll<(), io::Error> {
        self.inner.lock().poll_flush()
    }
}
