// Copyright 2021 Protocol Labs.
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

use crate::proto;
use async_trait::async_trait;
use futures::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use libp2p_core::{upgrade, Multiaddr, PeerId};
use libp2p_request_response::{self as request_response, ProtocolName};
use quick_protobuf::{BytesReader, Writer};
use std::{convert::TryFrom, io};

#[derive(Clone, Debug)]
pub struct AutoNatProtocol;

/// The protocol name used for negotiating with multistream-select.
pub const DEFAULT_PROTOCOL_NAME: &[u8] = b"/libp2p/autonat/1.0.0";

impl ProtocolName for AutoNatProtocol {
    fn protocol_name(&self) -> &[u8] {
        DEFAULT_PROTOCOL_NAME
    }
}

#[derive(Clone)]
pub struct AutoNatCodec;

#[async_trait]
impl request_response::Codec for AutoNatCodec {
    type Protocol = AutoNatProtocol;
    type Request = DialRequest;
    type Response = DialResponse;

    async fn read_request<T>(
        &mut self,
        _: &AutoNatProtocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Send + Unpin,
    {
        let bytes = upgrade::read_length_prefixed(io, 1024).await?;
        let request = DialRequest::from_bytes(&bytes)?;
        Ok(request)
    }

    async fn read_response<T>(
        &mut self,
        _: &AutoNatProtocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Send + Unpin,
    {
        let bytes = upgrade::read_length_prefixed(io, 1024).await?;
        let response = DialResponse::from_bytes(&bytes)?;
        Ok(response)
    }

    async fn write_request<T>(
        &mut self,
        _: &AutoNatProtocol,
        io: &mut T,
        data: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Send + Unpin,
    {
        upgrade::write_length_prefixed(io, data.into_bytes()).await?;
        io.close().await
    }

    async fn write_response<T>(
        &mut self,
        _: &AutoNatProtocol,
        io: &mut T,
        data: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Send + Unpin,
    {
        upgrade::write_length_prefixed(io, data.into_bytes()).await?;
        io.close().await
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialRequest {
    pub peer_id: PeerId,
    pub addresses: Vec<Multiaddr>,
}

impl DialRequest {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, io::Error> {
        use quick_protobuf::MessageRead;

        let mut reader = BytesReader::from_bytes(bytes);
        let msg = proto::Message::from_reader(&mut reader, bytes)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        if msg.type_pb != Some(proto::MessageType::DIAL) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid type"));
        }
        let (peer_id, addrs) = if let Some(proto::Dial {
            peer:
                Some(proto::PeerInfo {
                    id: Some(peer_id),
                    addrs,
                }),
        }) = msg.dial
        {
            (peer_id, addrs)
        } else {
            log::debug!("Received malformed dial message.");
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid dial message",
            ));
        };

        let peer_id = {
            PeerId::try_from(peer_id.to_vec())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid peer id"))?
        };

        let addrs = addrs
            .into_iter()
            .filter_map(|a| match Multiaddr::try_from(a.to_vec()) {
                Ok(a) => Some(a),
                Err(e) => {
                    log::debug!("Unable to parse multiaddr: {e}");
                    None
                }
            })
            .collect();
        Ok(Self {
            peer_id,
            addresses: addrs,
        })
    }

    pub fn into_bytes(self) -> Vec<u8> {
        use quick_protobuf::MessageWrite;

        let peer_id = self.peer_id.to_bytes();
        let addrs = self
            .addresses
            .into_iter()
            .map(|addr| addr.to_vec())
            .collect();

        let msg = proto::Message {
            type_pb: Some(proto::MessageType::DIAL),
            dial: Some(proto::Dial {
                peer: Some(proto::PeerInfo {
                    id: Some(peer_id.to_vec()),
                    addrs,
                }),
            }),
            dialResponse: None,
        };

        let mut buf = Vec::with_capacity(msg.get_size());
        let mut writer = Writer::new(&mut buf);
        msg.write_message(&mut writer).expect("Encoding to succeed");
        buf
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResponseError {
    DialError,
    DialRefused,
    BadRequest,
    InternalError,
}

impl From<ResponseError> for proto::ResponseStatus {
    fn from(t: ResponseError) -> Self {
        match t {
            ResponseError::DialError => proto::ResponseStatus::E_DIAL_ERROR,
            ResponseError::DialRefused => proto::ResponseStatus::E_DIAL_REFUSED,
            ResponseError::BadRequest => proto::ResponseStatus::E_BAD_REQUEST,
            ResponseError::InternalError => proto::ResponseStatus::E_INTERNAL_ERROR,
        }
    }
}

impl TryFrom<proto::ResponseStatus> for ResponseError {
    type Error = io::Error;

    fn try_from(value: proto::ResponseStatus) -> Result<Self, Self::Error> {
        match value {
            proto::ResponseStatus::E_DIAL_ERROR => Ok(ResponseError::DialError),
            proto::ResponseStatus::E_DIAL_REFUSED => Ok(ResponseError::DialRefused),
            proto::ResponseStatus::E_BAD_REQUEST => Ok(ResponseError::BadRequest),
            proto::ResponseStatus::E_INTERNAL_ERROR => Ok(ResponseError::InternalError),
            proto::ResponseStatus::OK => {
                log::debug!("Received response with status code OK but expected error.");
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid response error type",
                ))
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialResponse {
    pub status_text: Option<String>,
    pub result: Result<Multiaddr, ResponseError>,
}

impl DialResponse {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, io::Error> {
        use quick_protobuf::MessageRead;

        let mut reader = BytesReader::from_bytes(bytes);
        let msg = proto::Message::from_reader(&mut reader, bytes)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        if msg.type_pb != Some(proto::MessageType::DIAL_RESPONSE) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid type"));
        }

        Ok(match msg.dialResponse {
            Some(proto::DialResponse {
                status: Some(proto::ResponseStatus::OK),
                statusText,
                addr: Some(addr),
            }) => {
                let addr = Multiaddr::try_from(addr.to_vec())
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                Self {
                    status_text: statusText,
                    result: Ok(addr),
                }
            }
            Some(proto::DialResponse {
                status: Some(status),
                statusText,
                addr: None,
            }) => Self {
                status_text: statusText,
                result: Err(ResponseError::try_from(status)?),
            },
            _ => {
                log::debug!("Received malformed response message.");
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid dial response message",
                ));
            }
        })
    }

    pub fn into_bytes(self) -> Vec<u8> {
        use quick_protobuf::MessageWrite;

        let dial_response = match self.result {
            Ok(addr) => proto::DialResponse {
                status: Some(proto::ResponseStatus::OK),
                statusText: self.status_text,
                addr: Some(addr.to_vec()),
            },
            Err(error) => proto::DialResponse {
                status: Some(error.into()),
                statusText: self.status_text,
                addr: None,
            },
        };

        let msg = proto::Message {
            type_pb: Some(proto::MessageType::DIAL_RESPONSE),
            dial: None,
            dialResponse: Some(dial_response),
        };

        let mut buf = Vec::with_capacity(msg.get_size());
        let mut writer = Writer::new(&mut buf);
        msg.write_message(&mut writer).expect("Encoding to succeed");
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_protobuf::MessageWrite;

    #[test]
    fn test_request_encode_decode() {
        let request = DialRequest {
            peer_id: PeerId::random(),
            addresses: vec![
                "/ip4/8.8.8.8/tcp/30333".parse().unwrap(),
                "/ip4/192.168.1.42/tcp/30333".parse().unwrap(),
            ],
        };
        let bytes = request.clone().into_bytes();
        let request2 = DialRequest::from_bytes(&bytes).unwrap();
        assert_eq!(request, request2);
    }

    #[test]
    fn test_response_ok_encode_decode() {
        let response = DialResponse {
            result: Ok("/ip4/8.8.8.8/tcp/30333".parse().unwrap()),
            status_text: None,
        };
        let bytes = response.clone().into_bytes();
        let response2 = DialResponse::from_bytes(&bytes).unwrap();
        assert_eq!(response, response2);
    }

    #[test]
    fn test_response_err_encode_decode() {
        let response = DialResponse {
            result: Err(ResponseError::DialError),
            status_text: Some("dial failed".to_string()),
        };
        let bytes = response.clone().into_bytes();
        let response2 = DialResponse::from_bytes(&bytes).unwrap();
        assert_eq!(response, response2);
    }

    #[test]
    fn test_skip_unparsable_multiaddr() {
        let valid_multiaddr: Multiaddr = "/ip6/2001:db8::/tcp/1234".parse().unwrap();
        let valid_multiaddr_bytes = valid_multiaddr.to_vec();

        let invalid_multiaddr = {
            let a = vec![255; 8];
            assert!(Multiaddr::try_from(a.clone()).is_err());
            a
        };

        let msg = proto::Message {
            type_pb: Some(proto::MessageType::DIAL),
            dial: Some(proto::Dial {
                peer: Some(proto::PeerInfo {
                    id: Some(PeerId::random().to_bytes()),
                    addrs: vec![valid_multiaddr_bytes, invalid_multiaddr],
                }),
            }),
            dialResponse: None,
        };

        let mut bytes = Vec::with_capacity(msg.get_size());
        let mut writer = Writer::new(&mut bytes);
        msg.write_message(&mut writer).expect("Encoding to succeed");

        let request = DialRequest::from_bytes(&bytes).expect("not to fail");

        assert_eq!(request.addresses, vec![valid_multiaddr])
    }
}
