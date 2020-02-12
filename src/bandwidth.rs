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

use crate::{Multiaddr, core::{Transport, transport::{ListenerEvent, TransportError}}};
use futures::{prelude::*, io::{IoSlice, IoSliceMut}, ready};
use lazy_static::lazy_static;
use parking_lot::Mutex;
use smallvec::{smallvec, SmallVec};
use std::{cmp, io, pin::Pin, sync::Arc, task::{Context, Poll}, time::Duration};
use wasm_timer::Instant;

/// Wraps around a `Transport` and logs the bandwidth that goes through all the opened connections.
#[derive(Clone)]
pub struct BandwidthLogging<TInner> {
    inner: TInner,
    sinks: Arc<BandwidthSinks>,
}

impl<TInner> BandwidthLogging<TInner> {
    /// Creates a new `BandwidthLogging` around the transport.
    pub fn new(inner: TInner, period: Duration) -> (Self, Arc<BandwidthSinks>) {
        let mut period_seconds = cmp::min(period.as_secs(), 86400) as u32;
        if period.subsec_nanos() > 0 {
            period_seconds += 1;
        }

        let sink = Arc::new(BandwidthSinks {
            download: Mutex::new(BandwidthSink::new(period_seconds)),
            upload: Mutex::new(BandwidthSink::new(period_seconds)),
        });

        let trans = BandwidthLogging {
            inner,
            sinks: sink.clone(),
        };

        (trans, sink)
    }
}

impl<TInner> Transport for BandwidthLogging<TInner>
where
    TInner: Transport,
{
    type Output = BandwidthConnecLogging<TInner::Output>;
    type Error = TInner::Error;
    type Listener = BandwidthListener<TInner::Listener>;
    type ListenerUpgrade = BandwidthFuture<TInner::ListenerUpgrade>;
    type Dial = BandwidthFuture<TInner::Dial>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let sinks = self.sinks;
        self.inner
            .listen_on(addr)
            .map(move |inner| BandwidthListener { inner, sinks })
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let sinks = self.sinks;
        self.inner
            .dial(addr)
            .map(move |fut| BandwidthFuture { inner: fut, sinks })
    }
}

/// Wraps around a `Stream` that produces connections. Wraps each connection around a bandwidth
/// counter.
#[pin_project::pin_project]
pub struct BandwidthListener<TInner> {
    #[pin]
    inner: TInner,
    sinks: Arc<BandwidthSinks>,
}

impl<TInner, TConn, TErr> Stream for BandwidthListener<TInner>
where
    TInner: TryStream<Ok = ListenerEvent<TConn, TErr>, Error = TErr>
{
    type Item = Result<ListenerEvent<BandwidthFuture<TConn>, TErr>, TErr>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let this = self.project();

        let event =
            if let Some(event) = ready!(this.inner.try_poll_next(cx)?) {
                event
            } else {
                return Poll::Ready(None)
            };

        let event = event.map({
            let sinks = this.sinks.clone();
            |inner| BandwidthFuture { inner, sinks }
        });

        Poll::Ready(Some(Ok(event)))
    }
}

/// Wraps around a `Future` that produces a connection. Wraps the connection around a bandwidth
/// counter.
#[pin_project::pin_project]
pub struct BandwidthFuture<TInner> {
    #[pin]
    inner: TInner,
    sinks: Arc<BandwidthSinks>,
}

impl<TInner: TryFuture> Future for BandwidthFuture<TInner> {
    type Output = Result<BandwidthConnecLogging<TInner::Ok>, TInner::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.project();
        let inner = ready!(this.inner.try_poll(cx)?);
        let logged = BandwidthConnecLogging { inner, sinks: this.sinks.clone() };
        Poll::Ready(Ok(logged))
    }
}

/// Allows obtaining the average bandwidth of the connections created from a `BandwidthLogging`.
pub struct BandwidthSinks {
    download: Mutex<BandwidthSink>,
    upload: Mutex<BandwidthSink>,
}

impl BandwidthSinks {
    /// Returns the average number of bytes that have been downloaded in the period.
    pub fn average_download_per_sec(&self) -> u64 {
        self.download.lock().get()
    }

    /// Returns the average number of bytes that have been uploaded in the period.
    pub fn average_upload_per_sec(&self) -> u64 {
        self.upload.lock().get()
    }
}

/// Wraps around an `AsyncRead + AsyncWrite` and logs the bandwidth that goes through it.
#[pin_project::pin_project]
pub struct BandwidthConnecLogging<TInner> {
    #[pin]
    inner: TInner,
    sinks: Arc<BandwidthSinks>,
}

impl<TInner: AsyncRead> AsyncRead for BandwidthConnecLogging<TInner> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        let this = self.project();
        let num_bytes = ready!(this.inner.poll_read(cx, buf))?;
        this.sinks.download.lock().inject(num_bytes);
        Poll::Ready(Ok(num_bytes))
    }

    fn poll_read_vectored(self: Pin<&mut Self>, cx: &mut Context, bufs: &mut [IoSliceMut]) -> Poll<io::Result<usize>> {
        let this = self.project();
        let num_bytes = ready!(this.inner.poll_read_vectored(cx, bufs))?;
        this.sinks.download.lock().inject(num_bytes);
        Poll::Ready(Ok(num_bytes))
    }
}

impl<TInner: AsyncWrite> AsyncWrite for BandwidthConnecLogging<TInner> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.project();
        let num_bytes = ready!(this.inner.poll_write(cx, buf))?;
        this.sinks.upload.lock().inject(num_bytes);
        Poll::Ready(Ok(num_bytes))
    }

    fn poll_write_vectored(self: Pin<&mut Self>, cx: &mut Context, bufs: &[IoSlice]) -> Poll<io::Result<usize>> {
        let this = self.project();
        let num_bytes = ready!(this.inner.poll_write_vectored(cx, bufs))?;
        this.sinks.upload.lock().inject(num_bytes);
        Poll::Ready(Ok(num_bytes))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        let this = self.project();
        this.inner.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        let this = self.project();
        this.inner.poll_close(cx)
    }
}

/// Returns the number of seconds that have elapsed between an arbitrary EPOCH and now.
fn current_second() -> u32 {
    lazy_static! {
        static ref EPOCH: Instant = Instant::now();
    }

    EPOCH.elapsed().as_secs() as u32
}

/// Structure that calculates the average bandwidth over the last few seconds.
///
/// If you want to calculate for example both download and upload bandwidths, create two different
/// objects.
struct BandwidthSink {
    /// Bytes sent over the past seconds. Contains `rolling_seconds + 1` elements, where
    /// `rolling_seconds` is the value passed to `new`. Only the first `rolling_seconds` elements
    /// are taken into account for the average, while the last element is the element to be
    /// inserted later.
    bytes: SmallVec<[u64; 8]>,
    /// Number of seconds between `EPOCH` and the moment we have last updated `bytes`.
    latest_update: u32,
}

impl BandwidthSink {
    /// Initializes a `BandwidthSink`.
    fn new(seconds: u32) -> Self {
        BandwidthSink {
            bytes: smallvec![0; seconds as usize + 1],
            latest_update: current_second(),
        }
    }

    /// Returns the number of bytes over the last few seconds. The number of seconds is the value
    /// configured at initialization.
    fn get(&mut self) -> u64 {
        self.update();
        let seconds = self.bytes.len() - 1;
        self.bytes.iter()
            .take(seconds)
            .fold(0u64, |a, &b| a.saturating_add(b)) / seconds as u64
    }

    /// Notifies the `BandwidthSink` that a certain number of bytes have been transmitted at this
    /// moment.
    fn inject(&mut self, bytes: usize) {
        self.update();
        if let Some(last) = self.bytes.last_mut() {
            *last = last.saturating_add(bytes as u64);
        }
    }

    /// Updates the state of the `BandwidthSink` so that the last element of `bytes` contains the
    /// current second.
    fn update(&mut self) {
        let current_second = current_second();
        debug_assert!(current_second >= self.latest_update);
        let num_iter = cmp::min(current_second - self.latest_update, self.bytes.len() as u32);
        for _ in 0..num_iter {
            self.bytes.remove(0);
            self.bytes.push(0);
        }
        self.latest_update = current_second;
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};
    use super::*;

    #[test]
    fn sink_works() {
        let mut sink = BandwidthSink::new(5);
        sink.inject(100);
        thread::sleep(Duration::from_millis(1000));
        assert_eq!(sink.get(), 20);
        sink.inject(100);
        thread::sleep(Duration::from_millis(1000));
        assert_eq!(sink.get(), 40);
        sink.inject(100);
        thread::sleep(Duration::from_millis(1000));
        assert_eq!(sink.get(), 60);
        sink.inject(100);
        thread::sleep(Duration::from_millis(1000));
        assert_eq!(sink.get(), 80);
        sink.inject(100);
        thread::sleep(Duration::from_millis(1000));
        assert_eq!(sink.get(), 100);
        thread::sleep(Duration::from_millis(1000));
        assert_eq!(sink.get(), 80);
    }
}
