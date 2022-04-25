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

use crate::codec::MAX_FRAME_SIZE;
use std::cmp;

pub(crate) const DEFAULT_MPLEX_PROTOCOL_NAME: &[u8] = b"/mplex/6.7.0";

/// Configuration for the multiplexer.
#[derive(Debug, Clone)]
pub struct MplexConfig {
    /// Maximum number of simultaneously used substreams.
    pub(crate) max_substreams: usize,
    /// Maximum number of frames buffered per substream.
    pub(crate) max_buffer_len: usize,
    /// Behaviour when the buffer size limit is reached for a substream.
    pub(crate) max_buffer_behaviour: MaxBufferBehaviour,
    /// When sending data, split it into frames whose maximum size is this value
    /// (max 1MByte, as per the Mplex spec).
    pub(crate) split_send_size: usize,
    /// Protocol name, defaults to b"/mplex/6.7.0"
    pub(crate) protocol_name: &'static [u8],
}

impl MplexConfig {
    /// Builds the default configuration.
    pub fn new() -> MplexConfig {
        Default::default()
    }

    /// Sets the maximum number of simultaneously used substreams.
    ///
    /// A substream is used as long as it has not been dropped,
    /// even if it may already be closed or reset at the protocol
    /// level (in which case it may still have buffered data that
    /// can be read before the `StreamMuxer` API signals EOF).
    ///
    /// When the limit is reached, opening of outbound substreams
    /// is delayed until another substream is dropped, whereas new
    /// inbound substreams are immediately answered with a `Reset`.
    /// If the number of inbound substreams that need to be reset
    /// accumulates too quickly (judged by internal bounds), the
    /// connection is closed with an error due to the misbehaved
    /// remote.
    pub fn set_max_num_streams(&mut self, max: usize) -> &mut Self {
        self.max_substreams = max;
        self
    }

    /// Sets the maximum number of frames buffered per substream.
    ///
    /// A limit is necessary in order to avoid DoS attacks.
    pub fn set_max_buffer_size(&mut self, max: usize) -> &mut Self {
        self.max_buffer_len = max;
        self
    }

    /// Sets the behaviour when the maximum buffer size is reached
    /// for a substream.
    ///
    /// See the documentation of [`MaxBufferBehaviour`].
    pub fn set_max_buffer_behaviour(&mut self, behaviour: MaxBufferBehaviour) -> &mut Self {
        self.max_buffer_behaviour = behaviour;
        self
    }

    /// Sets the frame size used when sending data. Capped at 1Mbyte as per the
    /// Mplex spec.
    pub fn set_split_send_size(&mut self, size: usize) -> &mut Self {
        let size = cmp::min(size, MAX_FRAME_SIZE);
        self.split_send_size = size;
        self
    }

    /// Set the protocol name.
    ///
    /// ```rust
    /// use libp2p_mplex::MplexConfig;
    /// let mut muxer_config = MplexConfig::new();
    /// muxer_config.set_protocol_name(b"/mplex/6.7.0");
    /// ```
    pub fn set_protocol_name(&mut self, protocol_name: &'static [u8]) -> &mut Self {
        self.protocol_name = protocol_name;
        self
    }
}

/// Behaviour when the maximum length of the buffer is reached.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MaxBufferBehaviour {
    /// Reset the substream whose frame buffer overflowed.
    ///
    /// > **Note**: If more than [`MplexConfig::set_max_buffer_size()`] frames
    /// > are received in succession for a substream in the context of
    /// > trying to read data from a different substream, the former substream
    /// > may be reset before application code had a chance to read from the
    /// > buffer. The max. buffer size needs to be sized appropriately when
    /// > using this option to balance maximum resource usage and the
    /// > probability of premature termination of a substream.
    ResetStream,
    /// No new message can be read from the underlying connection from any
    /// substream as long as the buffer for a single substream is full,
    /// i.e. application code is expected to read from the full buffer.
    ///
    /// > **Note**: To avoid blocking without making progress, application
    /// > tasks should ensure that, when woken, always try to read (i.e.
    /// > make progress) from every substream on which data is expected.
    /// > This is imperative in general, as a woken task never knows for
    /// > which substream it has been woken, but failure to do so with
    /// > [`MaxBufferBehaviour::Block`] in particular may lead to stalled
    /// > execution or spinning of a task without progress.
    Block,
}

impl Default for MplexConfig {
    fn default() -> MplexConfig {
        MplexConfig {
            max_substreams: 128,
            max_buffer_len: 32,
            max_buffer_behaviour: MaxBufferBehaviour::Block,
            split_send_size: 8 * 1024,
            protocol_name: DEFAULT_MPLEX_PROTOCOL_NAME,
        }
    }
}
