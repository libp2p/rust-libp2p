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

use crate::structs_proto;
use async_trait::async_trait;
use futures::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use libp2p_core::{upgrade, Multiaddr, PeerId};
use libp2p_request_response::{ProtocolName, RequestResponseCodec};
use prost::Message;
use std::{convert::TryFrom, io};

#[derive(Clone, Debug)]
pub struct AutoNatProtocol;

impl ProtocolName for AutoNatProtocol {
    fn protocol_name(&self) -> &[u8] {
        b"/libp2p/autonat/1.0.0"
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
    pub addrs: Vec<Multiaddr>,
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
        Ok(Self { peer_id, addrs })
    }

    pub fn into_bytes(self) -> Vec<u8> {
        let peer_id = self.peer_id.to_bytes();
        let addrs = self.addrs.into_iter().map(|addr| addr.to_vec()).collect();

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

impl TryFrom<i32> for ResponseError {
    type Error = io::Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            100 => Ok(ResponseError::DialError),
            101 => Ok(ResponseError::DialRefused),
            200 => Ok(ResponseError::BadRequest),
            300 => Ok(ResponseError::InternalError),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid response error type",
            )),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DialResponse {
    Ok(Multiaddr),
    Err(ResponseError),
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
                status: Some(0),
                status_text: None,
                addr: Some(addr),
            }) => {
                let addr = Multiaddr::try_from(addr)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                Self::Ok(addr)
            }
            Some(structs_proto::message::DialResponse {
                status: Some(status),
                status_text: _,
                addr: None,
            }) => {
                let status = ResponseError::try_from(status)?;
                Self::Err(status)
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid dial response message",
                ));
            }
        })
    }

    pub fn into_bytes(self) -> Vec<u8> {
        let dial_response = match self {
            Self::Ok(addr) => structs_proto::message::DialResponse {
                status: Some(0),
                status_text: None,
                addr: Some(addr.to_vec()),
            },
            Self::Err(status) => structs_proto::message::DialResponse {
                status: Some(status.into()),
                status_text: None,
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
            addrs: vec![
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
        let response = DialResponse::Ok("/ip4/8.8.8.8/tcp/30333".parse().unwrap());
        let bytes = response.clone().into_bytes();
        let response2 = DialResponse::from_bytes(&bytes).unwrap();
        assert_eq!(response, response2);
    }

    #[test]
    fn test_response_err_encode_decode() {
        let response = DialResponse::Err(ResponseError::DialError);
        let bytes = response.clone().into_bytes();
        let response2 = DialResponse::from_bytes(&bytes).unwrap();
        assert_eq!(response, response2);
    }
}
