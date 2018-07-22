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

use futures::{future::Future, Poll, Async};
use parking_lot::Mutex;
use std::io::{Error as IoError, Read, Write};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Weak};
use tokio_io::{AsyncRead, AsyncWrite};

/// Implemented on objects that can be turned into a substream.
///
/// > **Note**: The methods of this trait consume the object, but if the object implements `Clone`
/// >           then you can clone it and keep the original in order to open additional substreams.
pub trait StreamMuxer {
    /// Type of the object that represents the raw substream where data can be read and written.
    type Substream: AsyncRead + AsyncWrite;

    /// Future that will be resolved when a new incoming substream is open.
    ///
    /// A `None` item signals that the underlying resource has been exhausted and
    /// no more substreams can be created.
    type InboundSubstream: Future<Item = Option<Self::Substream>, Error = IoError>;

    /// Future that will be resolved when the outgoing substream is open.
    ///
    /// A `None` item signals that the underlying resource has been exhausted and
    /// no more substreams can be created.
    type OutboundSubstream: Future<Item = Option<Self::Substream>, Error = IoError>;

    /// Produces a future that will be resolved when a new incoming substream arrives.
    fn inbound(self) -> Self::InboundSubstream;

    /// Opens a new outgoing substream, and produces a future that will be resolved when it becomes
    /// available.
    fn outbound(self) -> Self::OutboundSubstream;

    /// Wraps this muxer into an `AutoCloseStreamMuxer` so that it automatically closes the
    /// underlying stream muxer once all substreams are destroyed.
    #[inline]
    fn auto_close(self) -> AutoCloseStreamMuxer<Self>
        where Self: Sized
    {
        let arced = Arc::new(self);
        let weaked = Arc::downgrade(&arced);
        AutoCloseStreamMuxer {
            inner: weaked,
            keep_alive: Arc::new(Mutex::new(Some(arced))),
        }
    }
}

/// Wraps around a `StreamMuxer` and automatically kills the underlying muxer if all substreams
/// are dropped.
///
/// The exception is of course that at initialization we have 0 substreams but we don't immediately
/// kill the underlying muxer.
#[derive(Clone)]
pub struct AutoCloseStreamMuxer<M> {
    // The main pointer to the underlying muxer.
    inner: Weak<M>,
    // Holds the underlying muxer when we create the `AutoCloseStreamMuxer`. Emptied when we
    // open the first substream.
    keep_alive: Arc<Mutex<Option<Arc<M>>>>,
}

impl<M> StreamMuxer for AutoCloseStreamMuxer<M>
    where M: StreamMuxer + Clone
{
    type Substream = AutoCloseStreamMuxerSubstream<M>;
    type InboundSubstream = AutoCloseStreamMuxerInbound<M>;
    type OutboundSubstream = AutoCloseStreamMuxerOutbound<M>;

    #[inline]
    fn inbound(self) -> Self::InboundSubstream {
        match self.inner.upgrade() {
            Some(arced) => {
                let inner = (*arced).clone().inbound();
                AutoCloseStreamMuxerInbound {
                    inner: Some((inner, arced)),
                    keep_alive: self.keep_alive.clone(),
                }
            },
            None => {
                AutoCloseStreamMuxerInbound {
                    inner: None,
                    keep_alive: self.keep_alive.clone(),
                }
            }
        }
    }

    #[inline]
    fn outbound(self) -> Self::OutboundSubstream {
        match self.inner.upgrade() {
            Some(arced) => {
                let inner = (*arced).clone().outbound();
                AutoCloseStreamMuxerOutbound {
                    inner: Some((inner, arced)),
                    keep_alive: self.keep_alive.clone(),
                }
            },
            None => {
                AutoCloseStreamMuxerOutbound {
                    inner: None,
                    keep_alive: self.keep_alive.clone(),
                }
            }
        }
    }
}

/// Wraps around the `InboundSubstream` for the purpose of `AutoCloseStreamMuxer`.
pub struct AutoCloseStreamMuxerInbound<M>
    where M: StreamMuxer
{
    inner: Option<(M::InboundSubstream, Arc<M>)>,
    /// Holds the underlying muxer. Emptied when we manage to open the substream.
    keep_alive: Arc<Mutex<Option<Arc<M>>>>,
}

impl<M> Future for AutoCloseStreamMuxerInbound<M>
    where M: StreamMuxer
{
    type Item = Option<AutoCloseStreamMuxerSubstream<M>>;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner {
            Some(ref mut inner) => {
                let stream = match try_ready!(inner.0.poll()) {
                    Some(stream) => stream,
                    None => return Ok(Async::Ready(None)),
                };

                *self.keep_alive.lock() = None;
                Ok(Async::Ready(Some(AutoCloseStreamMuxerSubstream {
                    inner: stream,
                    _muxer_hold: inner.1.clone(),        // TODO: don't clone
                })))
            },
            None => {
                Ok(Async::Ready(None))
            }
        }
    }
}

/// Wraps around the `OutboundSubstream` for the purpose of `AutoCloseStreamMuxer`.
pub struct AutoCloseStreamMuxerOutbound<M>
    where M: StreamMuxer
{
    inner: Option<(M::OutboundSubstream, Arc<M>)>,
    /// Holds the underlying muxer. Emptied when we manage to open the substream.
    keep_alive: Arc<Mutex<Option<Arc<M>>>>,
}

impl<M> Future for AutoCloseStreamMuxerOutbound<M>
    where M: StreamMuxer
{
    type Item = Option<AutoCloseStreamMuxerSubstream<M>>;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner {
            Some(ref mut inner) => {
                let stream = match try_ready!(inner.0.poll()) {
                    Some(stream) => stream,
                    None => return Ok(Async::Ready(None)),
                };

                *self.keep_alive.lock() = None;
                Ok(Async::Ready(Some(AutoCloseStreamMuxerSubstream {
                    inner: stream,
                    _muxer_hold: inner.1.clone(),        // TODO: don't clone
                })))
            },
            None => {
                Ok(Async::Ready(None))
            }
        }
    }
}

/// Wraps around the `Substream` for the purpose of `AutoCloseStreamMuxer`.
pub struct AutoCloseStreamMuxerSubstream<M>
    where M: StreamMuxer
{
    inner: M::Substream,
    /// Holds the underlying muxer alive.
    _muxer_hold: Arc<M>,
}

impl<M> Deref for AutoCloseStreamMuxerSubstream<M>
    where M: StreamMuxer
{
    type Target = M::Substream;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<M> DerefMut for AutoCloseStreamMuxerSubstream<M>
    where M: StreamMuxer
{
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<M> Read for AutoCloseStreamMuxerSubstream<M>
    where M: StreamMuxer
{
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        self.inner.read(buf)
    }
}

impl<M> AsyncRead for AutoCloseStreamMuxerSubstream<M>
    where M: StreamMuxer
{
}

impl<M> Write for AutoCloseStreamMuxerSubstream<M>
    where M: StreamMuxer
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        self.inner.write(buf)
    }

    #[inline]
    fn flush(&mut self) -> Result<(), IoError> {
        self.inner.flush()
    }
}

impl<M> AsyncWrite for AutoCloseStreamMuxerSubstream<M>
    where M: StreamMuxer
{
    #[inline]
    fn shutdown(&mut self) -> Poll<(), IoError> {
        self.inner.shutdown()
    }
}
