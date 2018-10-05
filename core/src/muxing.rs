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

use fnv::FnvHashMap;
use futures::{future, prelude::*};
use parking_lot::Mutex;
use std::io::{Error as IoError, Read, Write};
use std::ops::Deref;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_io::{AsyncRead, AsyncWrite};

/// Implemented on objects that can open and manage substreams.
pub trait StreamMuxer {
    /// Type of the object that represents the raw substream where data can be read and written.
    type Substream;

    /// Future that will be resolved when the outgoing substream is open.
    type OutboundSubstream;

    /// Polls for an inbound substream.
    ///
    /// This function behaves the same as a `Stream`.
    ///
    /// If `NotReady` is returned, then the current task will be notified once the muxer
    /// is ready to be polled, similar to the API of `Stream::poll()`.
    /// However, only the latest task that was used to call this method may be notified.
    fn poll_inbound(&self) -> Poll<Option<Self::Substream>, IoError>;

    /// Opens a new outgoing substream, and produces a future that will be resolved when it becomes
    /// available.
    fn open_outbound(&self) -> Self::OutboundSubstream;

    /// Polls the outbound substream.
    ///
    /// If this returns `Ok(Ready(None))`, that means that the outbound channel is closed and that
    /// opening any further outbound substream will likely produce `None` as well. The existing
    /// outbound substream attempts may however still succeed.
    ///
    /// If `NotReady` is returned, then the current task will be notified once the substream
    /// is ready to be polled, similar to the API of `Future::poll()`.
    /// However, for each individual outbound substream, only the latest task that was used to
    /// call this method may be notified.
    ///
    /// May panic or produce an undefined result if an earlier polling of the same substream
    /// returned `Ready` or `Err`.
    fn poll_outbound(
        &self,
        substream: &mut Self::OutboundSubstream,
    ) -> Poll<Option<Self::Substream>, IoError>;

    /// Destroys an outbound substream. Use this after the outbound substream has finished, or if
    /// you want to interrupt it.
    fn destroy_outbound(&self, substream: Self::OutboundSubstream);

    /// Reads data from a substream. The behaviour is the same as `std::io::Read::read`.
    ///
    /// If `WouldBlock` is returned, then the current task will be notified once the substream
    /// is ready to be read, similar to the API of `AsyncRead`.
    /// However, for each individual substream, only the latest task that was used to call this
    /// method may be notified.
    fn read_substream(
        &self,
        substream: &mut Self::Substream,
        buf: &mut [u8],
    ) -> Result<usize, IoError>;

    /// Write data to a substream. The behaviour is the same as `std::io::Write::write`.
    ///
    /// If `WouldBlock` is returned, then the current task will be notified once the substream
    /// is ready to be written, similar to the API of `AsyncWrite`.
    /// However, for each individual substream, only the latest task that was used to call this
    /// method may be notified.
    fn write_substream(
        &self,
        substream: &mut Self::Substream,
        buf: &[u8],
    ) -> Result<usize, IoError>;

    /// Flushes a substream. The behaviour is the same as `std::io::Write::flush`.
    ///
    /// If `WouldBlock` is returned, then the current task will be notified once the substream
    /// is ready to be flushed, similar to the API of `AsyncWrite`.
    /// However, for each individual substream, only the latest task that was used to call this
    /// method may be notified.
    fn flush_substream(&self, substream: &mut Self::Substream) -> Result<(), IoError>;

    /// Attempts to shut down a substream. The behaviour is the same as
    /// `tokio_io::AsyncWrite::shutdown`.
    ///
    /// If `NotReady` is returned, then the current task will be notified once the substream
    /// is ready to be shut down, similar to the API of `AsyncWrite::shutdown()`.
    /// However, for each individual substream, only the latest task that was used to call this
    /// method may be notified.
    fn shutdown_substream(&self, substream: &mut Self::Substream) -> Poll<(), IoError>;

    /// Destroys a substream.
    fn destroy_substream(&self, substream: Self::Substream);

    /// If supported, sends a hint to the remote that we may no longer accept any further inbound
    /// substream. Calling `poll_inbound` afterwards may or may not produce `None`.
    fn close_inbound(&self);

    /// If supported, sends a hint to the remote that we may no longer open any further outbound
    /// substream. Calling `poll_outbound` afterwards may or may not produce `None`.
    fn close_outbound(&self);
}

/// Polls for an inbound from the muxer but wraps the output in an object that
/// implements `Read`/`Write`/`AsyncRead`/`AsyncWrite`.
#[inline]
pub fn inbound_from_ref_and_wrap<P>(
    muxer: P,
) -> impl Future<Item = Option<SubstreamRef<P>>, Error = IoError>
where
    P: Deref + Clone,
    P::Target: StreamMuxer,
{
    let muxer2 = muxer.clone();
    future::poll_fn(move || muxer.poll_inbound())
        .map(|substream| substream.map(move |s| substream_from_ref(muxer2, s)))
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
    type Item = Option<SubstreamRef<P>>;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll() {
            Ok(Async::Ready(Some(substream))) => {
                let out = substream_from_ref(self.inner.muxer.clone(), substream);
                Ok(Async::Ready(Some(out)))
            }
            Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
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
    type Item = Option<<P::Target as StreamMuxer>::Substream>;
    type Error = IoError;

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
{
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "Substream({:?})", self)
    }
}


impl<P> Read for SubstreamRef<P>
where
    P: Deref,
    P::Target: StreamMuxer,
{
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        self.muxer
            .read_substream(self.substream.as_mut().expect("substream was empty"), buf)
    }
}

impl<P> AsyncRead for SubstreamRef<P>
where
    P: Deref,
    P::Target: StreamMuxer,
{
}

impl<P> Write for SubstreamRef<P>
where
    P: Deref,
    P::Target: StreamMuxer,
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        self.muxer
            .write_substream(self.substream.as_mut().expect("substream was empty"), buf)
    }

    #[inline]
    fn flush(&mut self) -> Result<(), IoError> {
        self.muxer
            .flush_substream(self.substream.as_mut().expect("substream was empty"))
    }
}

impl<P> AsyncWrite for SubstreamRef<P>
where
    P: Deref,
    P::Target: StreamMuxer,
{
    #[inline]
    fn shutdown(&mut self) -> Poll<(), IoError> {
        self.muxer
            .shutdown_substream(self.substream.as_mut().expect("substream was empty"))
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
    inner: Box<StreamMuxer<Substream = usize, OutboundSubstream = usize> + Send + Sync>,
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

    #[inline]
    fn poll_inbound(&self) -> Poll<Option<Self::Substream>, IoError> {
        self.inner.poll_inbound()
    }

    #[inline]
    fn open_outbound(&self) -> Self::OutboundSubstream {
        self.inner.open_outbound()
    }

    #[inline]
    fn poll_outbound(
        &self,
        substream: &mut Self::OutboundSubstream,
    ) -> Poll<Option<Self::Substream>, IoError> {
        self.inner.poll_outbound(substream)
    }

    #[inline]
    fn destroy_outbound(&self, substream: Self::OutboundSubstream) {
        self.inner.destroy_outbound(substream)
    }

    #[inline]
    fn read_substream(
        &self,
        substream: &mut Self::Substream,
        buf: &mut [u8],
    ) -> Result<usize, IoError>
    {
        self.inner.read_substream(substream, buf)
    }

    #[inline]
    fn write_substream(
        &self,
        substream: &mut Self::Substream,
        buf: &[u8],
    ) -> Result<usize, IoError> {
        self.inner.write_substream(substream, buf)
    }

    #[inline]
    fn flush_substream(&self, substream: &mut Self::Substream) -> Result<(), IoError> {
        self.inner.flush_substream(substream)
    }

    #[inline]
    fn shutdown_substream(&self, substream: &mut Self::Substream) -> Poll<(), IoError> {
        self.inner.shutdown_substream(substream)
    }

    #[inline]
    fn destroy_substream(&self, substream: Self::Substream) {
        self.inner.destroy_substream(substream)
    }

    #[inline]
    fn close_inbound(&self) {
        self.inner.close_inbound()
    }

    #[inline]
    fn close_outbound(&self) {
        self.inner.close_outbound()
    }
}

struct Wrap<T> where T: StreamMuxer {
    inner: T,
    substreams: Mutex<FnvHashMap<usize, T::Substream>>,
    next_substream: AtomicUsize,
    outbound: Mutex<FnvHashMap<usize, T::OutboundSubstream>>,
    next_outbound: AtomicUsize,
}

impl<T> StreamMuxer for Wrap<T> where T: StreamMuxer {
    type Substream = usize; // TODO: use a newtype
    type OutboundSubstream = usize; // TODO: use a newtype

    #[inline]
    fn poll_inbound(&self) -> Poll<Option<Self::Substream>, IoError> {
        match try_ready!(self.inner.poll_inbound()) {
            Some(substream) => {
                let id = self.next_substream.fetch_add(1, Ordering::Relaxed);
                self.substreams.lock().insert(id, substream);
                Ok(Async::Ready(Some(id)))
            },
            None => Ok(Async::Ready(None)),
        }
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
    ) -> Poll<Option<Self::Substream>, IoError> {
        let mut list = self.outbound.lock();
        match try_ready!(self.inner.poll_outbound(list.get_mut(substream).unwrap())) {
            Some(substream) => {
                let id = self.next_substream.fetch_add(1, Ordering::Relaxed);
                self.substreams.lock().insert(id, substream);
                Ok(Async::Ready(Some(id)))
            },
            None => Ok(Async::Ready(None)),
        }
    }

    #[inline]
    fn destroy_outbound(&self, substream: Self::OutboundSubstream) {
        let mut list = self.outbound.lock();
        self.inner.destroy_outbound(list.remove(&substream).unwrap())
    }

    #[inline]
    fn read_substream(
        &self,
        substream: &mut Self::Substream,
        buf: &mut [u8],
    ) -> Result<usize, IoError>
    {
        let mut list = self.substreams.lock();
        self.inner.read_substream(list.get_mut(substream).unwrap(), buf)
    }

    #[inline]
    fn write_substream(
        &self,
        substream: &mut Self::Substream,
        buf: &[u8],
    ) -> Result<usize, IoError> {
        let mut list = self.substreams.lock();
        self.inner.write_substream(list.get_mut(substream).unwrap(), buf)
    }

    #[inline]
    fn flush_substream(&self, substream: &mut Self::Substream) -> Result<(), IoError> {
        let mut list = self.substreams.lock();
        self.inner.flush_substream(list.get_mut(substream).unwrap())
    }

    #[inline]
    fn shutdown_substream(&self, substream: &mut Self::Substream) -> Poll<(), IoError> {
        let mut list = self.substreams.lock();
        self.inner.shutdown_substream(list.get_mut(substream).unwrap())
    }

    #[inline]
    fn destroy_substream(&self, substream: Self::Substream) {
        let mut list = self.substreams.lock();
        self.inner.destroy_substream(list.remove(&substream).unwrap())
    }

    #[inline]
    fn close_inbound(&self) {
        self.inner.close_inbound()
    }

    #[inline]
    fn close_outbound(&self) {
        self.inner.close_outbound()
    }
}
