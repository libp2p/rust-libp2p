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

use libp2p_swarm::StreamProtocol;
use web_time::Duration;

pub mod client;
mod protocol;
pub mod server;

pub const PROTOCOL_NAME: StreamProtocol = StreamProtocol::new("/perf/1.0.0");
const RUN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MAX_PARALLEL_RUNS_PER_CONNECTION: usize = 1_000;

#[derive(Debug, Clone, Copy)]
pub enum RunUpdate {
    Intermediate(Intermediate),
    Final(Final),
}

#[derive(Debug, Clone, Copy)]
pub struct Intermediate {
    pub duration: Duration,
    pub sent: usize,
    pub received: usize,
}

impl Display for Intermediate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Intermediate {
            duration,
            sent,
            received,
        } = self;
        write!(
            f,
            "{:4} s uploaded {} downloaded {} ({})",
            duration.as_secs_f64(),
            format_bytes(*sent),
            format_bytes(*received),
            format_bandwidth(*duration, sent + received),
        )?;

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Final {
    pub duration: RunDuration,
}

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

#[derive(Debug, Clone, Copy)]
pub struct Run {
    pub params: RunParams,
    pub duration: RunDuration,
}

const KILO: f64 = 1024.0;
const MEGA: f64 = KILO * 1024.0;
const GIGA: f64 = MEGA * 1024.0;

fn format_bytes(bytes: usize) -> String {
    let bytes = bytes as f64;
    if bytes >= GIGA {
        format!("{:.2} GiB", bytes / GIGA)
    } else if bytes >= MEGA {
        format!("{:.2} MiB", bytes / MEGA)
    } else if bytes >= KILO {
        format!("{:.2} KiB", bytes / KILO)
    } else {
        format!("{} B", bytes)
    }
}

fn format_bandwidth(duration: Duration, bytes: usize) -> String {
    const KILO: f64 = 1024.0;
    const MEGA: f64 = KILO * 1024.0;
    const GIGA: f64 = MEGA * 1024.0;

    let bandwidth = (bytes as f64 * 8.0) / duration.as_secs_f64();

    if bandwidth >= GIGA {
        format!("{:.2} Gbit/s", bandwidth / GIGA)
    } else if bandwidth >= MEGA {
        format!("{:.2} Mbit/s", bandwidth / MEGA)
    } else if bandwidth >= KILO {
        format!("{:.2} Kbit/s", bandwidth / KILO)
    } else {
        format!("{:.2} bit/s", bandwidth)
    }
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

        write!(
            f,
            "uploaded {} in {:.4} s ({}), downloaded {} in {:.4} s ({})",
            format_bytes(*to_send),
            upload.as_secs_f64(),
            format_bandwidth(*upload, *to_send),
            format_bytes(*to_receive),
            download.as_secs_f64(),
            format_bandwidth(*download, *to_receive),
        )?;

        Ok(())
    }
}
