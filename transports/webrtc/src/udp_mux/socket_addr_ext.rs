// MIT License
//
// Copyright (c) 2021 WebRTC.rs
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use std::array::TryFromSliceError;
use std::convert::TryInto;
use std::net::SocketAddr;

use webrtc_util::Error;

pub(super) trait SocketAddrExt {
    ///Encode a representation of `self` into the buffer and return the length of this encoded
    ///version.
    ///
    /// The buffer needs to be at least 27 bytes in length.
    fn encode(&self, buffer: &mut [u8]) -> Result<usize, Error>;

    /// Decode a `SocketAddr` from a buffer. The encoding should have previously been done with
    /// [`SocketAddrExt::encode`].
    fn decode(buffer: &[u8]) -> Result<SocketAddr, Error>;
}

const IPV4_MARKER: u8 = 4;
const IPV4_ADDRESS_SIZE: usize = 7;
const IPV6_MARKER: u8 = 6;
const IPV6_ADDRESS_SIZE: usize = 27;

pub(super) const MAX_ADDR_SIZE: usize = IPV6_ADDRESS_SIZE;

impl SocketAddrExt for SocketAddr {
    fn encode(&self, buffer: &mut [u8]) -> Result<usize, Error> {
        use std::net::SocketAddr::*;

        if buffer.len() < MAX_ADDR_SIZE {
            return Err(Error::ErrBufferShort);
        }

        match self {
            V4(addr) => {
                let marker = IPV4_MARKER;
                let ip: [u8; 4] = addr.ip().octets();
                let port: u16 = addr.port();

                buffer[0] = marker;
                buffer[1..5].copy_from_slice(&ip);
                buffer[5..7].copy_from_slice(&port.to_le_bytes());

                Ok(7)
            },
            V6(addr) => {
                let marker = IPV6_MARKER;
                let ip: [u8; 16] = addr.ip().octets();
                let port: u16 = addr.port();
                let flowinfo = addr.flowinfo();
                let scope_id = addr.scope_id();

                buffer[0] = marker;
                buffer[1..17].copy_from_slice(&ip);
                buffer[17..19].copy_from_slice(&port.to_le_bytes());
                buffer[19..23].copy_from_slice(&flowinfo.to_le_bytes());
                buffer[23..27].copy_from_slice(&scope_id.to_le_bytes());

                Ok(MAX_ADDR_SIZE)
            },
        }
    }

    fn decode(buffer: &[u8]) -> Result<SocketAddr, Error> {
        use std::net::*;

        match buffer[0] {
            IPV4_MARKER => {
                if buffer.len() < IPV4_ADDRESS_SIZE {
                    return Err(Error::ErrBufferShort);
                }

                let ip_parts = &buffer[1..5];
                let port = match &buffer[5..7].try_into() {
                    Err(_) => return Err(Error::ErrFailedToParseIpaddr),
                    Ok(input) => u16::from_le_bytes(*input),
                };

                let ip = Ipv4Addr::new(ip_parts[0], ip_parts[1], ip_parts[2], ip_parts[3]);

                Ok(SocketAddr::V4(SocketAddrV4::new(ip, port)))
            },
            IPV6_MARKER => {
                if buffer.len() < IPV6_ADDRESS_SIZE {
                    return Err(Error::ErrBufferShort);
                }

                // Just to help the type system infer correctly
                fn helper(b: &[u8]) -> Result<&[u8; 16], TryFromSliceError> {
                    b.try_into()
                }

                let ip = match helper(&buffer[1..17]) {
                    Err(_) => return Err(Error::ErrFailedToParseIpaddr),
                    Ok(input) => Ipv6Addr::from(*input),
                };
                let port = match &buffer[17..19].try_into() {
                    Err(_) => return Err(Error::ErrFailedToParseIpaddr),
                    Ok(input) => u16::from_le_bytes(*input),
                };

                let flowinfo = match &buffer[19..23].try_into() {
                    Err(_) => return Err(Error::ErrFailedToParseIpaddr),
                    Ok(input) => u32::from_le_bytes(*input),
                };

                let scope_id = match &buffer[23..27].try_into() {
                    Err(_) => return Err(Error::ErrFailedToParseIpaddr),
                    Ok(input) => u32::from_le_bytes(*input),
                };

                Ok(SocketAddr::V6(SocketAddrV6::new(
                    ip, port, flowinfo, scope_id,
                )))
            },
            _ => Err(Error::ErrFailedToParseIpaddr),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::net::*;

    #[test]
    fn test_ipv4() {
        let ip = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from([56, 128, 35, 5]), 0x1234));

        let mut buffer = [0_u8; MAX_ADDR_SIZE];
        let encoded_len = ip.encode(&mut buffer);

        assert_eq!(encoded_len, Ok(7));
        assert_eq!(
            &buffer[0..7],
            &[IPV4_MARKER, 56, 128, 35, 5, 0x34, 0x12][..]
        );

        let decoded = SocketAddr::decode(&buffer);

        assert_eq!(decoded, Ok(ip));
    }

    #[test]
    fn test_ipv6() {
        let ip = SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::from([
                92, 114, 235, 3, 244, 64, 38, 111, 20, 100, 199, 241, 19, 174, 220, 123,
            ]),
            0x1234,
            0x12345678,
            0x87654321,
        ));

        let mut buffer = [0_u8; MAX_ADDR_SIZE];
        let encoded_len = ip.encode(&mut buffer);

        assert_eq!(encoded_len, Ok(27));
        assert_eq!(
            &buffer[0..27],
            &[
                IPV6_MARKER, // marker
                // Start of ipv6 address
                92,
                114,
                235,
                3,
                244,
                64,
                38,
                111,
                20,
                100,
                199,
                241,
                19,
                174,
                220,
                123,
                // LE port
                0x34,
                0x12,
                // LE flowinfo
                0x78,
                0x56,
                0x34,
                0x12,
                // LE scope_id
                0x21,
                0x43,
                0x65,
                0x87,
            ][..]
        );

        let decoded = SocketAddr::decode(&buffer);

        assert_eq!(decoded, Ok(ip));
    }

    #[test]
    fn test_encode_ipv4_with_short_buffer() {
        let mut buffer = vec![0u8; IPV4_ADDRESS_SIZE - 1];
        let ip = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from([56, 128, 35, 5]), 0x1234));

        let result = ip.encode(&mut buffer);

        assert_eq!(result, Err(Error::ErrBufferShort));
    }

    #[test]
    fn test_encode_ipv6_with_short_buffer() {
        let mut buffer = vec![0u8; MAX_ADDR_SIZE - 1];
        let ip = SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::from([
                92, 114, 235, 3, 244, 64, 38, 111, 20, 100, 199, 241, 19, 174, 220, 123,
            ]),
            0x1234,
            0x12345678,
            0x87654321,
        ));

        let result = ip.encode(&mut buffer);

        assert_eq!(result, Err(Error::ErrBufferShort));
    }

    #[test]
    fn test_decode_ipv4_with_short_buffer() {
        let buffer = vec![IPV4_MARKER, 0];

        let result = SocketAddr::decode(&buffer);

        assert_eq!(result, Err(Error::ErrBufferShort));
    }

    #[test]
    fn test_decode_ipv6_with_short_buffer() {
        let buffer = vec![IPV6_MARKER, 0];

        let result = SocketAddr::decode(&buffer);

        assert_eq!(result, Err(Error::ErrBufferShort));
    }
}
