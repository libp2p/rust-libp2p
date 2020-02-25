// Copyright 2017-2020 Parity Technologies (UK) Ltd.
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

//! The state of all active streams in a QUIC connection

use super::stream::StreamState;
use futures::channel::oneshot;
use std::collections::HashMap;
use std::task;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd)]
/// A stream ID.
pub(super) struct StreamId(quinn_proto::StreamId);

impl std::ops::Deref for StreamId {
    type Target = quinn_proto::StreamId;
    fn deref(&self) -> &quinn_proto::StreamId {
        &self.0
    }
}

/// A set of streams.
#[derive(Debug, Default)]
pub(super) struct Streams {
    map: HashMap<quinn_proto::StreamId, StreamState>,
}

impl Streams {
    pub(super) fn add_stream(&mut self, id: quinn_proto::StreamId) -> StreamId {
        if self.map.insert(id, Default::default()).is_some() {
            panic!(
                "Internal state corrupted. \
                You probably used a Substream with the wrong StreamMuxer",
            )
        }
        StreamId(id)
    }

    fn get(&mut self, id: &StreamId) -> &mut StreamState {
        self.map.get_mut(id).expect(
            "Internal state corrupted. \
            You probably used a Substream with the wrong StreamMuxer",
        )
    }

    /// Indicate that the stream is open for reading. Calling this when nobody
    /// is waiting for this stream to be readable is a harmless no-op.
    pub(super) fn wake_reader(&mut self, id: quinn_proto::StreamId) {
        if let Some(stream) = self.map.get_mut(&id) {
            stream.wake_reader()
        }
    }

    /// If a task is waiting for this stream to be finished or written to, wake
    /// it up. Otherwise, do nothing.
    pub(super) fn wake_writer(&mut self, id: quinn_proto::StreamId) {
        if let Some(stream) = self.map.get_mut(&id) {
            stream.wake_writer()
        }
    }

    /// Set a waker that will be notified when the state becomes readable.
    /// Wake up any waker that has already been registered.
    pub(super) fn set_reader(&mut self, id: &StreamId, waker: task::Waker) {
        self.get(id).set_reader(waker);
    }

    /// Set a waker that will be notified when the task becomes writable or is
    /// finished, waking up any waker or channel that has already been
    /// registered.
    ///
    /// # Panics
    ///
    /// Panics if the stream has already been finished.
    pub(super) fn set_writer(&mut self, id: &StreamId, waker: task::Waker) {
        self.get(id).set_writer(waker);
    }

    /// Set a channel that will be notified when the task becomes writable or is
    /// finished, waking up any existing registered waker or channel.
    ///
    /// # Panics
    ///
    /// Panics if the stream has already been finished.
    pub(super) fn set_finisher(&mut self, id: &StreamId, finisher: oneshot::Sender<()>) {
        self.get(id).set_finisher(finisher);
    }

    /// Remove an ID from the map.
    ///
    /// # Panics
    ///
    /// Panics if the ID has already been removed.
    pub(super) fn remove(&mut self, id: StreamId) {
        self.map.remove(&id.0).expect(
            "Internal state corrupted. \
                You probably used a Substream with the wrong StreamMuxer",
        );
    }

    /// Wake all wakers and call the provided callback for each stream,
    /// so as to free resources.
    ///
    /// # Panics
    ///
    /// Panics if [`Self::close`] has already been called.
    pub(super) fn close<T: FnMut(quinn_proto::StreamId)>(&mut self, mut cb: T) {
        for (stream, value) in &mut self.map {
            value.wake_all();
            cb(*stream)
        }
    }

    /// Wake up everything
    pub(super) fn wake_all(&mut self) {
        for value in self.map.values_mut() {
            value.wake_all()
        }
    }
}
