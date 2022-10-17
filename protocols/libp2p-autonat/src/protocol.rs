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

use crate::structs_proto;
use async_trait::async_trait;
use futures::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use libp2p_core::{upgrade, Multiaddr, PeerId};
use libp2p_request_response::{ProtocolName, RequestResponseCodec};
use prost::Message;
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
impl RequestResponseCodec for AutoNatCodec {
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
        let msg = structs_proto::Message::decode(bytes)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        if msg.r#type != Some(structs_proto::message::MessageType::Dial as _) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid type"));
        }
        let (peer_id, addrs) = if let Some(structs_proto::message::Dial {
            peer:
                Some(structs_proto::message::PeerInfo {
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
            PeerId::try_from(peer_id)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid peer id"))?
        };
        let addrs = {
            let mut maddrs = vec![];
            for addr in addrs.into_iter() {
                let maddr = Multiaddr::try_from(addr)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                maddrs.push(maddr);
            }
            maddrs
        };
        Ok(Self {
            peer_id,
            addresses: addrs,
        })
    }

    pub fn into_bytes(self) -> Vec<u8> {
        let peer_id = self.peer_id.to_bytes();
        let addrs = self
            .addresses
            .into_iter()
            .map(|addr| addr.to_vec())
            .collect();

        let msg = structs_proto::Message {
            r#type: Some(structs_proto::message::MessageType::Dial as _),
            dial: Some(structs_proto::message::Dial {
                peer: Some(structs_proto::message::PeerInfo {
                    id: Some(peer_id),
                    addrs,
                }),
            }),
            dial_response: None,
        };

        let mut bytes = Vec::with_capacity(msg.encoded_len());
        msg.encode(&mut bytes)
            .expect("Vec<u8> provides capacity as needed");
        bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResponseError {
    DialError,
    DialRefused,
    BadRequest,
    InternalError,
}

impl From<ResponseError> for i32 {
    fn from(t: ResponseError) -> Self {
        match t {
            ResponseError::DialError => 100,
            ResponseError::DialRefused => 101,
            ResponseError::BadRequest => 200,
            ResponseError::InternalError => 300,
        }
    }
}

impl TryFrom<structs_proto::message::ResponseStatus> for ResponseError {
    type Error = io::Error;

    fn try_from(value: structs_proto::message::ResponseStatus) -> Result<Self, Self::Error> {
        match value {
            structs_proto::message::ResponseStatus::EDialError => Ok(ResponseError::DialError),
            structs_proto::message::ResponseStatus::EDialRefused => Ok(ResponseError::DialRefused),
            structs_proto::message::ResponseStatus::EBadRequest => Ok(ResponseError::BadRequest),
            structs_proto::message::ResponseStatus::EInternalError => {
                Ok(ResponseError::InternalError)
            }
            structs_proto::message::ResponseStatus::Ok => {
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
        let msg = structs_proto::Message::decode(bytes)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        if msg.r#type != Some(structs_proto::message::MessageType::DialResponse as _) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid type"));
        }

        Ok(match msg.dial_response {
            Some(structs_proto::message::DialResponse {
                status: Some(status),
                status_text,
                addr: Some(addr),
            }) if structs_proto::message::ResponseStatus::from_i32(status)
                == Some(structs_proto::message::ResponseStatus::Ok) =>
            {
                let addr = Multiaddr::try_from(addr)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                Self {
                    status_text,
                    result: Ok(addr),
                }
            }
            Some(structs_proto::message::DialResponse {
                status: Some(status),
                status_text,
                addr: None,
            }) => Self {
                status_text,
                result: Err(ResponseError::try_from(
                    structs_proto::message::ResponseStatus::from_i32(status).ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid response status code")
                    })?,
                )?),
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
        let dial_response = match self.result {
            Ok(addr) => structs_proto::message::DialResponse {
                status: Some(0),
                status_text: self.status_text,
                addr: Some(addr.to_vec()),
            },
            Err(error) => structs_proto::message::DialResponse {
                status: Some(error.into()),
                status_text: self.status_text,
                addr: None,
            },
        };

        let msg = structs_proto::Message {
            r#type: Some(structs_proto::message::MessageType::DialResponse as _),
            dial: None,
            dial_response: Some(dial_response),
        };

        let mut bytes = Vec::with_capacity(msg.encoded_len());
        msg.encode(&mut bytes)
            .expect("Vec<u8> provides capacity as needed");
        bytes
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
}
