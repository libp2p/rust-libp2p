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

//! Combines scanning for nearby devices with filtering them depending on whether they support
//! libp2p.

use crate::{LIBP2P_UUID, LIBP2P_PEER_ID_ATTRIB, Addr, hci_scan::HciScan, sdp_client};
use futures::prelude::*;
use libp2p_core::{Multiaddr, PeerId};
use std::{io, iter};

/// A scan in progress.
pub struct Scan {
    /// The stream for scanning devices. If `None`, it means that the scan is finished.
    devices: Option<HciScan>,
    /// Clients for querying whether devices support.
    sdp_scans: Vec<sdp_client::SdpClient>,
}

impl Scan {
    /// Initializes a new scan.
    pub fn new() -> Result<Scan, io::Error> {
        Ok(Scan {
            devices: Some(HciScan::new()?),
            sdp_scans: Vec::new(),
        })
    }

    /// Pulls the discovered devices, or `None` if we have finished enumerating.
    ///
    /// Each device will only appear once in the list.
    ///
    /// Just like `Stream::poll()`, must be executed within the context of a task. If `NotReady` is
    /// returned, the current task is registered then notified when something is ready.
    pub fn poll(&mut self) -> Poll<Option<(Multiaddr, PeerId)>, io::Error> {
        loop {
            match self.devices.as_mut().map(|d| d.poll()) {
                Some(Ok(Async::Ready(Some(addr)))) => {
                    let mut client = sdp_client::SdpClient::connect(addr)?;
                    client.start_request(LIBP2P_UUID, iter::once(sdp_client::AttributeSearch::Attribute(LIBP2P_PEER_ID_ATTRIB)));
                    self.sdp_scans.push(client);
                },
                Some(Ok(Async::Ready(None))) => { self.devices = None; break; },
                Some(Err(err)) => return Err(err),
                None | Some(Ok(Async::NotReady)) => break,
            };
        }

        for n in (0..self.sdp_scans.len()).rev() {
            let mut in_progress = self.sdp_scans.swap_remove(n);
            match in_progress.poll()? {
                Async::Ready(sdp_client::SdpClientEvent::RequestFailed { .. }) => {
                    panic!()
                },
                Async::Ready(sdp_client::SdpClientEvent::RequestSuccess { results, .. }) => {
                    // TODO:
                },
                Async::NotReady => {
                    self.sdp_scans.push(in_progress);
                }
            }
        }

        if self.sdp_scans.is_empty() && self.devices.is_none() {
            Ok(Async::Ready(None))
        } else {
            Ok(Async::NotReady)
        }
    }
}


