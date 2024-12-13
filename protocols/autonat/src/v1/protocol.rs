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

use std::io;

use async_trait::async_trait;
use asynchronous_codec::{FramedRead, FramedWrite};
use futures::{
    io::{AsyncRead, AsyncWrite},
    SinkExt, StreamExt,
};
use libp2p_core::Multiaddr;
use libp2p_identity::PeerId;
use libp2p_request_response::{self as request_response};
use libp2p_swarm::StreamProtocol;

use crate::proto;

/// The protocol name used for negotiating with multistream-select.
pub const DEFAULT_PROTOCOL_NAME: StreamProtocol = StreamProtocol::new("/libp2p/autonat/1.0.0");

#[derive(Clone)]
pub struct AutoNatCodec;

#[async_trait]
impl request_response::Codec for AutoNatCodec {
    type Protocol = StreamProtocol;
    type Request = DialRequest;
    type Response = DialResponse;

    async fn read_request<T>(&mut self, _: &StreamProtocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Send + Unpin,
    {
        let message = FramedRead::new(io, codec())
            .next()
            .await
            .ok_or(io::ErrorKind::UnexpectedEof)??;
        let request = DialRequest::from_proto(message)?;

        Ok(request)
    }

    async fn read_response<T>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Send + Unpin,
    {
        let message = FramedRead::new(io, codec())
            .next()
            .await
            .ok_or(io::ErrorKind::UnexpectedEof)??;
        let response = DialResponse::from_proto(message)?;

        Ok(response)
    }

    async fn write_request<T>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
        data: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Send + Unpin,
    {
        let mut framed = FramedWrite::new(io, codec());
        framed.send(data.into_proto()).await?;
        framed.close().await?;

        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _: &StreamProtocol,
        io: &mut T,
        data: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Send + Unpin,
    {
        let mut framed = FramedWrite::new(io, codec());
        framed.send(data.into_proto()).await?;
        framed.close().await?;

        Ok(())
    }
}

fn codec() -> quick_protobuf_codec::Codec<proto::Message> {
    quick_protobuf_codec::Codec::<proto::Message>::new(1024)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialRequest {
    pub peer_id: PeerId,
    pub addresses: Vec<Multiaddr>,
}

impl DialRequest {
    pub fn from_proto(msg: proto::Message) -> Result<Self, io::Error> {
        if msg.type_pb != Some(proto::MessageType::DIAL) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid type"));
        }

        let peer_id_result = msg.dial.and_then(|dial| {
            dial.peer
                .and_then(|peer_info| peer_info.id.map(|peer_id| (peer_id, peer_info.addrs)))
        });

        let (peer_id, addrs) = peer_id_result
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid dial message"))?;

        let peer_id = {
            PeerId::try_from(peer_id.to_vec())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid peer id"))?
        };

        let addrs = addrs
            .into_iter()
            .filter_map(|a| match Multiaddr::try_from(a.to_vec()) {
                Ok(a) => Some(a),
                Err(e) => {
                    tracing::debug!("Unable to parse multiaddr: {e}");
                    None
                }
            })
            .collect();
        Ok(Self {
            peer_id,
            addresses: addrs,
        })
    }

    pub fn into_proto(self) -> proto::Message {
        let peer_id = self.peer_id.to_bytes();
        let addrs = self
            .addresses
            .into_iter()
            .map(|addr| addr.to_vec())
            .collect();

        proto::Message {
            type_pb: Some(proto::MessageType::DIAL),
            dial: Some(proto::Dial {
                peer: Some(proto::PeerInfo {
                    id: Some(peer_id.to_vec()),
                    addrs,
                }),
            }),
            dialResponse: None,
        }
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
                tracing::debug!("Received response with status code OK but expected error");
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
    pub fn from_proto(msg: proto::Message) -> Result<Self, io::Error> {
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
                tracing::debug!("Received malformed response message");
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid dial response message",
                ));
            }
        })
    }

    pub fn into_proto(self) -> proto::Message {
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

        proto::Message {
            type_pb: Some(proto::MessageType::DIAL_RESPONSE),
            dial: None,
            dialResponse: Some(dial_response),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_encode_decode() {
        let request = DialRequest {
            peer_id: PeerId::random(),
            addresses: vec![
                "/ip4/8.8.8.8/tcp/30333".parse().unwrap(),
                "/ip4/192.168.1.42/tcp/30333".parse().unwrap(),
            ],
        };
        let proto = request.clone().into_proto();
        let request2 = DialRequest::from_proto(proto).unwrap();
        assert_eq!(request, request2);
    }

    #[test]
    fn test_response_ok_encode_decode() {
        let response = DialResponse {
            result: Ok("/ip4/8.8.8.8/tcp/30333".parse().unwrap()),
            status_text: None,
        };
        let proto = response.clone().into_proto();
        let response2 = DialResponse::from_proto(proto).unwrap();
        assert_eq!(response, response2);
    }

    #[test]
    fn test_response_err_encode_decode() {
        let response = DialResponse {
            result: Err(ResponseError::DialError),
            status_text: Some("dial failed".to_string()),
        };
        let proto = response.clone().into_proto();
        let response2 = DialResponse::from_proto(proto).unwrap();
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

        let request = DialRequest::from_proto(msg).expect("not to fail");

        assert_eq!(request.addresses, vec![valid_multiaddr])
    }
}
