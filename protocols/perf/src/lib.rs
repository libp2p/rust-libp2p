// Copyright 2023 Protocol Labs.
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

//! Implementation of the [libp2p perf protocol](https://github.com/libp2p/specs/pull/478/).
//!
//! Do not use in untrusted environments.

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

use std::fmt::Display;

use instant::Duration;
use libp2p_swarm::StreamProtocol;

pub mod client;
mod protocol;
pub mod server;

pub const PROTOCOL_NAME: StreamProtocol = StreamProtocol::new("/perf/1.0.0");

/// Parameters for a single run, i.e. one stream, sending and receiving data.
///
/// Property names are from the perspective of the actor. E.g. `to_send` is the amount of data to
/// send, both as the client and the server.
#[derive(Debug, Clone, Copy)]
pub struct RunParams {
    pub to_send: usize,
    pub to_receive: usize,
}

/// Duration for a single run, i.e. one stream, sending and receiving data.
#[derive(Debug, Clone, Copy)]
pub struct RunDuration {
    pub upload: Duration,
    pub download: Duration,
}

struct Run {
    params: RunParams,
    duration: RunDuration,
}

impl Display for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Run {
            params: RunParams {
                to_send,
                to_receive,
            },
            duration: RunDuration { upload, download },
        } = self;
        let upload_seconds = upload.as_secs_f64();
        let download_seconds = download.as_secs_f64();

        let sent_mebibytes = *to_send as f64 / 1024.0 / 1024.0;
        let sent_bandwidth_mebibit_second = (sent_mebibytes * 8.0) / upload_seconds;

        let received_mebibytes = *to_receive as f64 / 1024.0 / 1024.0;
        let receive_bandwidth_mebibit_second = (received_mebibytes * 8.0) / download_seconds;

        write!(
            f,
            "uploaded {to_send} in {upload_seconds:.4} ({sent_bandwidth_mebibit_second} MiBit/s), downloaded {to_receive} in {download_seconds} ({receive_bandwidth_mebibit_second} MiBit/s)",
        )?;

        Ok(())
    }
}
