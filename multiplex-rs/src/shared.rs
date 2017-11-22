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

use read::MultiplexReadState;
use write::MultiplexWriteState;

use std::collections::HashMap;
use bytes::Bytes;
use arrayvec::ArrayVec;
use futures::task::Task;

const BUF_SIZE: usize = 1024;

pub type ByteBuf = ArrayVec<[u8; BUF_SIZE]>;

pub enum SubstreamMetadata {
    Closed,
    Open {
        read: Option<Task>,
        write: Option<Task>,
    },
}

impl SubstreamMetadata {
    pub fn open(&self) -> bool {
        match *self {
            SubstreamMetadata::Closed => false,
            SubstreamMetadata::Open { .. } => true,
        }
    }

    pub fn read_task(&self) -> Option<&Task> {
        match *self {
            SubstreamMetadata::Closed => None,
            SubstreamMetadata::Open { ref read, .. } => read.as_ref(),
        }
    }

    pub fn write_task(&self) -> Option<&Task> {
        match *self {
            SubstreamMetadata::Closed => None,
            SubstreamMetadata::Open { ref write, .. } => write.as_ref(),
        }
    }
}

// TODO: Split reading and writing into different structs and have information shared between the
//       two in a `RwLock`, since `open_streams` and `to_open` are mostly read-only.
pub struct MultiplexShared<T> {
    // We use `Option` in order to take ownership of heap allocations within `DecoderState` and
    // `BytesMut`. If this is ever observably `None` then something has panicked or the underlying
    // stream returned an error.
    pub read_state: Option<MultiplexReadState>,
    pub write_state: Option<MultiplexWriteState>,
    pub stream: T,
    // true if the stream is open, false otherwise
    pub open_streams: HashMap<u32, SubstreamMetadata>,
    // TODO: Should we use a version of this with a fixed size that doesn't allocate and return
    //       `WouldBlock` if it's full?
    pub to_open: HashMap<u32, Option<Bytes>>,
}

impl<T> MultiplexShared<T> {
    pub fn new(stream: T) -> Self {
        MultiplexShared {
            read_state: Default::default(),
            write_state: Default::default(),
            open_streams: Default::default(),
            to_open: Default::default(),
            stream: stream,
        }
    }

    pub fn open_stream(&mut self, id: u32) -> bool {
        self.open_streams
            .entry(id)
            .or_insert(SubstreamMetadata::Open {
                read: None,
                write: None,
            })
            .open()
    }

    pub fn close_stream(&mut self, id: u32) {
        self.open_streams.insert(id, SubstreamMetadata::Closed);
    }
}

pub fn buf_from_slice(slice: &[u8]) -> ByteBuf {
    slice.iter().cloned().take(BUF_SIZE).collect()
}
