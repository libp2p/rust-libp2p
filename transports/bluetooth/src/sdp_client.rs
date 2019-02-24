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

//! Documented in the Bluetooth core specifications, Volume 3 part B.

use super::l2cap;
use crate::Addr;
use byteorder::{WriteBytesExt, BigEndian, LittleEndian};
use std::{io, mem, os::raw::c_int, os::raw::c_void};

const PUBLIC_BROWSE_GROUP: u16 = 0x1002;
//#define SDP_RESPONSE_TIMEOUT	20
const SDP_REQ_BUFFER_SIZE: usize = 2048;
const SDP_RSP_BUFFER_SIZE: usize = 65535;
const SDP_PDU_CHUNK_SIZE: usize = 1024;

const SDP_SVC_SEARCH_ATTR_REQ: u8 = 0x06;
const SDP_SVC_SEARCH_ATTR_RSP: u8 = 0x07;

/// Non-blocking socket for SDP queries with a remote.
pub struct SdpClient {
    socket: tokio_reactor::PollEvented<l2cap::L2capSocket>,
    next_request_id: u16,
}

impl SdpClient {
    pub fn connect(addr: Addr) -> Result<SdpClient, io::Error> {
        let socket = l2cap::L2capSocket::new()?;
        socket.connect(addr, 0x1)?;
        Ok(SdpClient {
            socket: tokio_reactor::PollEvented::new(socket),
            next_request_id: 0,
        })
    }

    pub fn rq(&mut self) {
        // Assign an ID for the request.
        let request_id = {
            let i = self.next_request_id;
            // TODO: wrapping_add? really?
            self.next_request_id = self.next_request_id.wrapping_add(1);
            i
        };

        let mut rq_data = Vec::new();
        rq_data.write_u8(0x04).unwrap();
        rq_data.write_u16::<BigEndian>(request_id).unwrap();
        rq_data.write_u16::<BigEndian>(14).unwrap();      // rest of packet len in bytes
        rq_data.write_u32::<BigEndian>(0x00010000).unwrap();
        rq_data.write_u16::<BigEndian>(0xffff).unwrap();
        rq_data.write_u8(0x35).unwrap();
        rq_data.write_u8(0x05).unwrap();
        rq_data.write_u8(0x0a).unwrap();
        rq_data.write_u32::<BigEndian>(0x0000ffff).unwrap();
        rq_data.write_u8(0x00).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::SdpClient;

    #[test]
    fn test() {
        let client = SdpClient::connect("3C:77:E6:F0:FD:A2".parse().unwrap());
    }
}
