// Copyright 2018 Parity Technologies (UK) Ltd.
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

//! Muxing is the process of splitting a connection into multiple substreams.
//!
//! The main item of this module is the `StreamMuxer` trait. An implementation of `StreamMuxer`
//! has ownership of a connection, lets you open and close substreams, and read/write data
//! on open substreams.
//!
//! > **Note**: You normally don't need to use the methods of the `StreamMuxer` directly, as this
//! >           is managed by the library's internals.
//!
//! Each substream of a connection is an isolated stream of data. All the substreams are muxed
//! together so that the data read from or written to each substream doesn't influence the other
//! substreams.
//!
//! In the context of libp2p, each substream can use a different protocol. Contrary to opening a
//! connection, opening a substream is almost free in terms of resources. This means that you
//! shouldn't hesitate to rapidly open and close substreams, and to design protocols that don't
//! require maintaining long-lived channels of communication.
//!
//! > **Example**: The Kademlia protocol opens a new substream for each request it wants to
//! >              perform. Multiple requests can be performed simultaneously by opening multiple
//! >              substreams, without having to worry about associating responses with the
//! >              right request.
//!
//! # Implementing a muxing protocol
//!
//! In order to implement a muxing protocol, create an object that implements the `UpgradeInfo`,
//! `InboundUpgrade` and `OutboundUpgrade` traits. See the `upgrade` module for more information.
//! The `Output` associated type of the `InboundUpgrade` and `OutboundUpgrade` traits should be
//! identical, and should be an object that implements the `StreamMuxer` trait.
//!
//! The upgrade process will take ownership of the connection, which makes it possible for the
//! implementation of `StreamMuxer` to control everything that happens on the wire.

use fnv::FnvHashMap;
use futures::{future, prelude::*, try_ready};
use parking_lot::Mutex;
use std::fmt;
use std::io::{self, Read, Write};
use std::ops::Deref;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_io::{AsyncRead, AsyncWrite};

pub use self::singleton::SingletonMuxer;

mod singleton;

/// Implemented on objects that can open and manage substreams.
///
/// The state of a muxer, as exposed by this API, is the following:
///
/// - A connection to the remote. The `is_remote_acknowledged`, `flush_all` and `close` methods
///   operate on this.
/// - A list of substreams that are open. The `poll_inbound`, `poll_outbound`, `read_substream`,
///   `write_substream`, `flush_substream`, `shutdown_substream` and `destroy_substream` methods
///   allow controlling these entries.
/// - A list of outbound substreams being opened. The `open_outbound`, `poll_outbound` and
///   `destroy_outbound` methods allow controlling these entries.
///
pub trait StreamMuxer {
    /// Type of the object that represents the raw substream where data can be read and written.
    type Substream;

    /// Future that will be resolved when the outgoing substream is open.
    type OutboundSubstream;

    /// Error type of the muxer
    type Error: Into<io::Error>;

    /// Polls for an inbound substream.
    ///
    /// This function behaves the same as a `Stream`.
    ///
    /// If `NotReady` is returned, then the current task will be notified once the muxer
    /// is ready to be polled, similar to the API of `Stream::poll()`.
    /// Only the latest task that was used to call this method may be notified.
    ///
    /// An error can be generated if the connection has been closed.
    fn poll_inbound(&self) -> Poll<Self::Substream, Self::Error>;

    /// Opens a new outgoing substream, and produces the equivalent to a future that will be
    /// resolved when it becomes available.
    ///
    /// The API of `OutboundSubstream` is totally opaque, and the object can only be interfaced
    /// through the methods on the `StreamMuxer` trait.
    fn open_outbound(&self) -> Self::OutboundSubstream;

    /// Polls the outbound substream.
    ///
    /// If `NotReady` is returned, then the current task will be notified once the substream
    /// is ready to be polled, similar to the API of `Future::poll()`.
    /// However, for each individual outbound substream, only the latest task that was used to
    /// call this method may be notified.
    ///
    /// May panic or produce an undefined result if an earlier polling of the same substream
    /// returned `Ready` or `Err`.
    fn poll_outbound(&self, s: &mut Self::OutboundSubstream) -> Poll<Self::Substream, Self::Error>;

    /// Destroys an outbound substream future. Use this after the outbound substream has finished,
    /// or if you want to interrupt it.
    fn destroy_outbound(&self, s: Self::OutboundSubstream);

    /// Reads data from a substream. The behaviour is the same as `tokio_io::AsyncRead::poll_read`.
    ///
    /// If `NotReady` is returned, then the current task will be notified once the substream
    /// is ready to be read. However, for each individual substream, only the latest task that
    /// was used to call this method may be notified.
    ///
    /// If `Async::Ready(0)` is returned, the substream has been closed by the remote and should
    /// no longer be read afterwards.
    ///
    /// An error can be generated if the connection has been closed, or if a protocol misbehaviour
    /// happened.
    fn read_substream(&self, s: &mut Self::Substream, buf: &mut [u8]) -> Poll<usize, Self::Error>;

    /// Mimics the `prepare_uninitialized_buffer` method of the `AsyncRead` trait.
    ///
    /// This function isn't actually unsafe to call but unsafe to implement. The implementer must
    /// ensure that either the whole buf has been zeroed or that `read_substream` overwrites the
    /// buffer without reading it and returns correct value.
    ///
    /// If this function returns true, then the memory has been zeroed out. This allows
    /// implementations of `AsyncRead` which are composed of multiple subimplementations to
    /// efficiently implement `prepare_uninitialized_buffer`.
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        for b in buf.iter_mut() {
            *b = 0;
        }
        true
    }

    /// Write data to a substream. The behaviour is the same as `tokio_io::AsyncWrite::poll_write`.
    ///
    /// If `NotReady` is returned, then the current task will be notified once the substream
    /// is ready to be read. For each individual substream, only the latest task that was used to
    /// call this method may be notified.
    ///
    /// Calling `write_substream` does not guarantee that data will arrive to the remote. To
    /// ensure that, you should call `flush_substream`.
    ///
    /// It is incorrect to call this method on a substream if you called `shutdown_substream` on
    /// this substream earlier.
    fn write_substream(&self, s: &mut Self::Substream, buf: &[u8]) -> Poll<usize, Self::Error>;

    /// Flushes a substream. The behaviour is the same as `tokio_io::AsyncWrite::poll_flush`.
    ///
    /// After this method has been called, data written earlier on the substream is guaranteed to
    /// be received by the remote.
    ///
    /// If `NotReady` is returned, then the current task will be notified once the substream
    /// is ready to be read. For each individual substream, only the latest task that was used to
    /// call this method may be notified.
    ///
    /// > **Note**: This method may be implemented as a call to `flush_all`.
    fn flush_substream(&self, s: &mut Self::Substream) -> Poll<(), Self::Error>;

    /// Attempts to shut down the writing side of a substream. The behaviour is similar to
    /// `tokio_io::AsyncWrite::shutdown`.
    ///
    /// Contrary to `AsyncWrite::shutdown`, shutting down a substream does not imply
    /// `flush_substream`. If you want to make sure that the remote is immediately informed about
    /// the shutdown, use `flush_substream` or `flush_all`.
    ///
    /// After this method has been called, you should no longer attempt to write to this substream.
    ///
    /// An error can be generated if the connection has been closed, or if a protocol misbehaviour
    /// happened.
    fn shutdown_substream(&self, s: &mut Self::Substream) -> Poll<(), Self::Error>;

    /// Destroys a substream.
    fn destroy_substream(&self, s: Self::Substream);

    /// Returns `true` if the remote has shown any sign of activity after the muxer has been open.
    ///
    /// For optimisation purposes, the connection handshake of libp2p can be very optimistic and is
    /// allowed to assume that the handshake has succeeded when it didn't in fact succeed. This
    /// method can be called in order to determine whether the remote has accepted our handshake or
    /// has potentially not received it yet.
    fn is_remote_acknowledged(&self) -> bool;

    /// Closes this `StreamMuxer`.
    ///
    /// After this has returned `Ok(Async::Ready(()))`, the muxer has become useless. All
    /// subsequent reads must return either `EOF` or an error. All subsequent writes, shutdowns,
    /// or polls must generate an error or be ignored.
    ///
    /// Calling this method implies `flush_all`.
    ///
    /// > **Note**: You are encouraged to call this method and wait for it to return `Ready`, so
    /// >           that the remote is properly informed of the shutdown. However, apart from
    /// >           properly informing the remote, there is no difference between this and
    /// >           immediately dropping the muxer.
    fn close(&self) -> Poll<(), Self::Error>;

    /// Flush this `StreamMuxer`.
    ///
    /// This drains any write buffers of substreams and delivers any pending shutdown notifications
    /// due to `shutdown_substream` or `close`. One may thus shutdown groups of substreams
    /// followed by a final `flush_all` instead of having to do `flush_substream` for each.
    fn flush_all(&self) -> Poll<(), Self::Error>;
}

/// Polls for an inbound from the muxer but wraps the output in an object that
/// implements `Read`/`Write`/`AsyncRead`/`AsyncWrite`.
#[inline]
pub fn inbound_from_ref_and_wrap<P>(
    muxer: P,
) -> impl Future<Item = SubstreamRef<P>, Error = <P::Target as StreamMuxer>::Error>
where
    P: Deref + Clone,
    P::Target: StreamMuxer,
{
    let muxer2 = muxer.clone();
    future::poll_fn(move || muxer.poll_inbound())
        .map(|substream| substream_from_ref(muxer2, substream))
}

/// Same as `outbound_from_ref`, but wraps the output in an object that
/// implements `Read`/`Write`/`AsyncRead`/`AsyncWrite`.
#[inline]
pub fn outbound_from_ref_and_wrap<P>(muxer: P) -> OutboundSubstreamRefWrapFuture<P>
where
    P: Deref + Clone,
    P::Target: StreamMuxer,
{
    let inner = outbound_from_ref(muxer);
    OutboundSubstreamRefWrapFuture { inner }
}

/// Future returned by `outbound_from_ref_and_wrap`.
pub struct OutboundSubstreamRefWrapFuture<P>
where
    P: Deref + Clone,
    P::Target: StreamMuxer,
{
    inner: OutboundSubstreamRefFuture<P>,
}

impl<P> Future for OutboundSubstreamRefWrapFuture<P>
where
    P: Deref + Clone,
    P::Target: StreamMuxer,
{
    type Item = SubstreamRef<P>;
    type Error = <P::Target as StreamMuxer>::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll() {
            Ok(Async::Ready(substream)) => {
                let out = substream_from_ref(self.inner.muxer.clone(), substream);
                Ok(Async::Ready(out))
            }
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(err) => Err(err),
        }
    }
}

/// Builds a new future for an outbound substream, where the muxer is a reference.
#[inline]
pub fn outbound_from_ref<P>(muxer: P) -> OutboundSubstreamRefFuture<P>
where
    P: Deref,
    P::Target: StreamMuxer,
{
    let outbound = muxer.open_outbound();
    OutboundSubstreamRefFuture {
        muxer,
        outbound: Some(outbound),
    }
}

/// Future returned by `outbound_from_ref`.
pub struct OutboundSubstreamRefFuture<P>
where
    P: Deref,
    P::Target: StreamMuxer,
{
    muxer: P,
    outbound: Option<<P::Target as StreamMuxer>::OutboundSubstream>,
}

impl<P> Future for OutboundSubstreamRefFuture<P>
where
    P: Deref,
    P::Target: StreamMuxer,
{
    type Item = <P::Target as StreamMuxer>::Substream;
    type Error = <P::Target as StreamMuxer>::Error;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.muxer
            .poll_outbound(self.outbound.as_mut().expect("outbound was empty"))
    }
}

impl<P> Drop for OutboundSubstreamRefFuture<P>
where
    P: Deref,
    P::Target: StreamMuxer,
{
    #[inline]
    fn drop(&mut self) {
        self.muxer
            .destroy_outbound(self.outbound.take().expect("outbound was empty"))
    }
}

/// Builds an implementation of `Read`/`Write`/`AsyncRead`/`AsyncWrite` from an `Arc` to the
/// muxer and a substream.
#[inline]
pub fn substream_from_ref<P>(
    muxer: P,
    substream: <P::Target as StreamMuxer>::Substream,
) -> SubstreamRef<P>
where
    P: Deref,
    P::Target: StreamMuxer,
{
    SubstreamRef {
        muxer,
        substream: Some(substream),
    }
}

/// Stream returned by `substream_from_ref`.
pub struct SubstreamRef<P>
where
    P: Deref,
    P::Target: StreamMuxer,
{
    muxer: P,
    substream: Option<<P::Target as StreamMuxer>::Substream>,
}

impl<P> fmt::Debug for SubstreamRef<P>
where
    P: Deref,
    P::Target: StreamMuxer,
    <P::Target as StreamMuxer>::Substream: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "Substream({:?})", self.substream)
    }
}

impl<P> Read for SubstreamRef<P>
where
    P: Deref,
    P::Target: StreamMuxer,
{
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        let s = self.substream.as_mut().expect("substream was empty");
        match self.muxer.read_substream(s, buf).map_err(|e| e.into())? {
            Async::Ready(n) => Ok(n),
            Async::NotReady => Err(io::ErrorKind::WouldBlock.into()),
        }
    }
}

impl<P> AsyncRead for SubstreamRef<P>
where
    P: Deref,
    P::Target: StreamMuxer,
{
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.muxer.prepare_uninitialized_buffer(buf)
    }

    fn poll_read(&mut self, buf: &mut [u8]) -> Poll<usize, io::Error> {
        let s = self.substream.as_mut().expect("substream was empty");
        self.muxer.read_substream(s, buf).map_err(|e| e.into())
    }
}

impl<P> Write for SubstreamRef<P>
where
    P: Deref,
    P::Target: StreamMuxer,
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        let s = self.substream.as_mut().expect("substream was empty");
        match self.muxer.write_substream(s, buf).map_err(|e| e.into())? {
            Async::Ready(n) => Ok(n),
            Async::NotReady => Err(io::ErrorKind::WouldBlock.into()),
        }
    }

    #[inline]
    fn flush(&mut self) -> Result<(), io::Error> {
        let s = self.substream.as_mut().expect("substream was empty");
        match self.muxer.flush_substream(s).map_err(|e| e.into())? {
            Async::Ready(()) => Ok(()),
            Async::NotReady => Err(io::ErrorKind::WouldBlock.into()),
        }
    }
}

impl<P> AsyncWrite for SubstreamRef<P>
where
    P: Deref,
    P::Target: StreamMuxer,
{
    #[inline]
    fn poll_write(&mut self, buf: &[u8]) -> Poll<usize, io::Error> {
        let s = self.substream.as_mut().expect("substream was empty");
        self.muxer.write_substream(s, buf).map_err(|e| e.into())
    }

    #[inline]
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        let s = self.substream.as_mut().expect("substream was empty");
        self.muxer.shutdown_substream(s).map_err(|e| e.into())?;
        Ok(Async::Ready(()))
    }

    #[inline]
    fn poll_flush(&mut self) -> Poll<(), io::Error> {
        let s = self.substream.as_mut().expect("substream was empty");
        self.muxer.flush_substream(s).map_err(|e| e.into())
    }
}

impl<P> Drop for SubstreamRef<P>
where
    P: Deref,
    P::Target: StreamMuxer,
{
    #[inline]
    fn drop(&mut self) {
        self.muxer
            .destroy_substream(self.substream.take().expect("substream was empty"))
    }
}

/// Abstract `StreamMuxer`.
pub struct StreamMuxerBox {
    inner: Box<
        dyn StreamMuxer<Substream = usize, OutboundSubstream = usize, Error = io::Error>
            + Send
            + Sync,
    >,
}

impl StreamMuxerBox {
    /// Turns a stream muxer into a `StreamMuxerBox`.
    pub fn new<T>(muxer: T) -> StreamMuxerBox
    where
        T: StreamMuxer + Send + Sync + 'static,
        T::OutboundSubstream: Send,
        T::Substream: Send,
    {
        let wrap = Wrap {
            inner: muxer,
            substreams: Mutex::new(Default::default()),
            next_substream: AtomicUsize::new(0),
            outbound: Mutex::new(Default::default()),
            next_outbound: AtomicUsize::new(0),
        };

        StreamMuxerBox {
            inner: Box::new(wrap),
        }
    }
}

impl StreamMuxer for StreamMuxerBox {
    type Substream = usize; // TODO: use a newtype
    type OutboundSubstream = usize; // TODO: use a newtype
    type Error = io::Error;

    #[inline]
    fn poll_inbound(&self) -> Poll<Self::Substream, Self::Error> {
        self.inner.poll_inbound()
    }

    #[inline]
    fn open_outbound(&self) -> Self::OutboundSubstream {
        self.inner.open_outbound()
    }

    #[inline]
    fn poll_outbound(&self, s: &mut Self::OutboundSubstream) -> Poll<Self::Substream, Self::Error> {
        self.inner.poll_outbound(s)
    }

    #[inline]
    fn destroy_outbound(&self, substream: Self::OutboundSubstream) {
        self.inner.destroy_outbound(substream)
    }

    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.inner.prepare_uninitialized_buffer(buf)
    }

    #[inline]
    fn read_substream(&self, s: &mut Self::Substream, buf: &mut [u8]) -> Poll<usize, Self::Error> {
        self.inner.read_substream(s, buf)
    }

    #[inline]
    fn write_substream(&self, s: &mut Self::Substream, buf: &[u8]) -> Poll<usize, Self::Error> {
        self.inner.write_substream(s, buf)
    }

    #[inline]
    fn flush_substream(&self, s: &mut Self::Substream) -> Poll<(), Self::Error> {
        self.inner.flush_substream(s)
    }

    #[inline]
    fn shutdown_substream(&self, s: &mut Self::Substream) -> Poll<(), Self::Error> {
        self.inner.shutdown_substream(s)
    }

    #[inline]
    fn destroy_substream(&self, s: Self::Substream) {
        self.inner.destroy_substream(s)
    }

    #[inline]
    fn close(&self) -> Poll<(), Self::Error> {
        self.inner.close()
    }

    #[inline]
    fn is_remote_acknowledged(&self) -> bool {
        self.inner.is_remote_acknowledged()
    }

    #[inline]
    fn flush_all(&self) -> Poll<(), Self::Error> {
        self.inner.flush_all()
    }
}

struct Wrap<T>
where
    T: StreamMuxer,
{
    inner: T,
    substreams: Mutex<FnvHashMap<usize, T::Substream>>,
    next_substream: AtomicUsize,
    outbound: Mutex<FnvHashMap<usize, T::OutboundSubstream>>,
    next_outbound: AtomicUsize,
}

impl<T> StreamMuxer for Wrap<T>
where
    T: StreamMuxer,
{
    type Substream = usize; // TODO: use a newtype
    type OutboundSubstream = usize; // TODO: use a newtype
    type Error = io::Error;

    #[inline]
    fn poll_inbound(&self) -> Poll<Self::Substream, Self::Error> {
        let substream = try_ready!(self.inner.poll_inbound().map_err(|e| e.into()));
        let id = self.next_substream.fetch_add(1, Ordering::Relaxed);
        self.substreams.lock().insert(id, substream);
        Ok(Async::Ready(id))
    }

    #[inline]
    fn open_outbound(&self) -> Self::OutboundSubstream {
        let outbound = self.inner.open_outbound();
        let id = self.next_outbound.fetch_add(1, Ordering::Relaxed);
        self.outbound.lock().insert(id, outbound);
        id
    }

    #[inline]
    fn poll_outbound(
        &self,
        substream: &mut Self::OutboundSubstream,
    ) -> Poll<Self::Substream, Self::Error> {
        let mut list = self.outbound.lock();
        let substream = try_ready!(self
            .inner
            .poll_outbound(list.get_mut(substream).unwrap())
            .map_err(|e| e.into()));
        let id = self.next_substream.fetch_add(1, Ordering::Relaxed);
        self.substreams.lock().insert(id, substream);
        Ok(Async::Ready(id))
    }

    #[inline]
    fn destroy_outbound(&self, substream: Self::OutboundSubstream) {
        let mut list = self.outbound.lock();
        self.inner
            .destroy_outbound(list.remove(&substream).unwrap())
    }

    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.inner.prepare_uninitialized_buffer(buf)
    }

    #[inline]
    fn read_substream(&self, s: &mut Self::Substream, buf: &mut [u8]) -> Poll<usize, Self::Error> {
        let mut list = self.substreams.lock();
        self.inner
            .read_substream(list.get_mut(s).unwrap(), buf)
            .map_err(|e| e.into())
    }

    #[inline]
    fn write_substream(&self, s: &mut Self::Substream, buf: &[u8]) -> Poll<usize, Self::Error> {
        let mut list = self.substreams.lock();
        self.inner
            .write_substream(list.get_mut(s).unwrap(), buf)
            .map_err(|e| e.into())
    }

    #[inline]
    fn flush_substream(&self, s: &mut Self::Substream) -> Poll<(), Self::Error> {
        let mut list = self.substreams.lock();
        self.inner
            .flush_substream(list.get_mut(s).unwrap())
            .map_err(|e| e.into())
    }

    #[inline]
    fn shutdown_substream(&self, s: &mut Self::Substream) -> Poll<(), Self::Error> {
        let mut list = self.substreams.lock();
        self.inner
            .shutdown_substream(list.get_mut(s).unwrap())
            .map_err(|e| e.into())
    }

    #[inline]
    fn destroy_substream(&self, substream: Self::Substream) {
        let mut list = self.substreams.lock();
        self.inner
            .destroy_substream(list.remove(&substream).unwrap())
    }

    #[inline]
    fn close(&self) -> Poll<(), Self::Error> {
        self.inner.close().map_err(|e| e.into())
    }

    #[inline]
    fn is_remote_acknowledged(&self) -> bool {
        self.inner.is_remote_acknowledged()
    }

    #[inline]
    fn flush_all(&self) -> Poll<(), Self::Error> {
        self.inner.flush_all().map_err(|e| e.into())
    }
}
