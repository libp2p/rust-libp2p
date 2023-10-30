use std::{borrow::Cow, io, sync::OnceLock};

use futures::{AsyncRead, AsyncWrite, AsyncWriteExt};
use libp2p_core::{
    upgrade::{read_length_prefixed, write_length_prefixed},
    Multiaddr,
};
use libp2p_request_response::{Behaviour, Codec};
use libp2p_swarm::{ConnectionId, NetworkBehaviour, StreamProtocol};
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};
use rand::Rng;

use crate::generated::structs as proto;

const REQUEST_MAX_SIZE: usize = 4104;
const DATA_LEN_LOWER_BOUND: usize = 30_000u32 as usize;
const DATA_LEN_UPPER_BOUND: usize = 100_000u32 as usize;
const DATA_FIELD_LEN_UPPER_BOUND: usize = 4096;

pub(crate) const REQUEST_PROTOCOL_NAME: StreamProtocol =
    StreamProtocol::new("/libp2p/autonat/2/dial-request");

macro_rules! new_io_invalid_data_err {
    ($msg:expr) => {
        io::Error::new(io::ErrorKind::InvalidData, $msg)
    };
}

macro_rules! check_existence {
    ($field:ident) => {
        $field.ok_or_else(|| new_io_invalid_data_err!(concat!(stringify!($field), " is missing")))
    };
}

#[derive(Debug, Clone)]
pub(crate) enum Request {
    Dial(DialRequest),
    Data(DialDataResponse),
}

#[derive(Debug, Clone)]
pub(crate) struct DialRequest {
    pub(crate) addrs: Vec<Multiaddr>,
    pub(crate) nonce: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct DialDataResponse {
    pub(crate) data_count: usize,
}

impl Request {
    fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        let mut reader = BytesReader::from_bytes(bytes);
        let msg = proto::Message::from_reader(&mut reader, bytes)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        match msg.msg {
            proto::mod_Message::OneOfmsg::dialRequest(proto::DialRequest { addrs, nonce }) => {
                let addrs: Vec<Multiaddr> = addrs
                    .into_iter()
                    .map(|e| e.to_vec())
                    .map(|e| {
                        Multiaddr::try_from(e).map_err(|err| {
                            new_io_invalid_data_err!(format!("invalid multiaddr: {}", err))
                        })
                    })
                    .collect::<Result<Vec<_>, io::Error>>()?;
                let nonce = check_existence!(nonce)?;
                Ok(Self::Dial(DialRequest { addrs, nonce }))
            }
            proto::mod_Message::OneOfmsg::dialDataResponse(proto::DialDataResponse { data }) => {
                let data_count = check_existence!(data)?.len();
                Ok(Self::Data(DialDataResponse { data_count }))
            }
            _ => Err(new_io_invalid_data_err!(
                "invalid message type, expected dialRequest or dialDataResponse"
            )),
        }
    }

    fn into_bytes(self) -> Cow<'static, [u8]> {
        fn make_message_bytes(request: Request) -> Vec<u8> {
            let msg = match request {
                Request::Dial(DialRequest { addrs, nonce }) => {
                    let addrs = addrs.iter().map(|e| e.to_vec().into()).collect();
                    let nonce = Some(nonce);
                    proto::Message {
                        msg: proto::mod_Message::OneOfmsg::dialRequest(proto::DialRequest {
                            addrs,
                            nonce,
                        }),
                    }
                }
                Request::Data(DialDataResponse { data_count }) => {
                    assert!(
                        data_count <= DATA_FIELD_LEN_UPPER_BOUND,
                        "data_count too large"
                    );
                    static DATA: &[u8] = &[0u8; DATA_FIELD_LEN_UPPER_BOUND];
                    proto::Message {
                        msg: proto::mod_Message::OneOfmsg::dialDataResponse(
                            proto::DialDataResponse {
                                data: Some(Cow::Borrowed(&DATA[..data_count])),
                            },
                        ),
                    }
                }
            };
            let mut buf = Vec::with_capacity(msg.get_size());
            let mut writer = Writer::new(&mut buf);
            msg.write_message(&mut writer).expect("encoding to succeed");
            buf
        }
        // little optimization: if the data is exactly 4096 bytes, we can use a static buffer. It is
        // likely that this is the case, draining the most performance.
        if matches!(
            self,
            Self::Data(DialDataResponse {
                data_count: DATA_FIELD_LEN_UPPER_BOUND
            })
        ) {
            static CELL: OnceLock<Vec<u8>> = OnceLock::new();
            CELL.get_or_init(move || make_message_bytes(self)).into()
        } else {
            make_message_bytes(self).into()
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Response {
    Dial(DialResponse),
    Data(DialDataRequest),
}

#[derive(Debug, Clone)]
pub(crate) struct DialDataRequest {
    pub(crate) addr_idx: usize,
    pub(crate) num_bytes: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct DialResponse {
    pub(crate) status: proto::mod_DialResponse::ResponseStatus,
    pub(crate) addr_idx: usize,
    pub(crate) dial_status: proto::DialStatus,
}

impl Response {
    fn from_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        let mut reader = BytesReader::from_bytes(bytes);
        let msg = proto::Message::from_reader(&mut reader, bytes)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

        match msg.msg {
            proto::mod_Message::OneOfmsg::dialResponse(proto::DialResponse {
                status,
                addrIdx,
                dialStatus,
            }) => {
                let status = check_existence!(status)?;
                let addr_idx = check_existence!(addrIdx)? as usize;
                let dial_status = check_existence!(dialStatus)?;
                Ok(Self::Dial(DialResponse {
                    status,
                    addr_idx,
                    dial_status,
                }))
            }
            proto::mod_Message::OneOfmsg::dialDataRequest(proto::DialDataRequest {
                addrIdx,
                numBytes,
            }) => {
                let addr_idx = check_existence!(addrIdx)? as usize;
                let num_bytes = check_existence!(numBytes)? as usize;
                Ok(Self::Data(DialDataRequest {
                    addr_idx,
                    num_bytes,
                }))
            }
            _ => Err(new_io_invalid_data_err!(
                "invalid message type, expected dialResponse or dialDataRequest"
            )),
        }
    }

    fn into_bytes(self) -> Vec<u8> {
        let msg = match self {
            Self::Dial(DialResponse {
                status,
                addr_idx,
                dial_status,
            }) => proto::Message {
                msg: proto::mod_Message::OneOfmsg::dialResponse(proto::DialResponse {
                    status: Some(status),
                    addrIdx: Some(addr_idx as u32),
                    dialStatus: Some(dial_status),
                }),
            },
            Self::Data(DialDataRequest {
                addr_idx,
                num_bytes,
            }) => proto::Message {
                msg: proto::mod_Message::OneOfmsg::dialDataRequest(proto::DialDataRequest {
                    addrIdx: Some(addr_idx as u32),
                    numBytes: Some(num_bytes as u64),
                }),
            },
        };
        let mut buf = Vec::with_capacity(msg.get_size());
        let mut writer = Writer::new(&mut buf);
        msg.write_message(&mut writer).expect("encoding to succeed");
        buf
    }
}

impl DialDataRequest {
    pub(crate) fn from_rng<R: rand_core::RngCore>(addr_idx: usize, mut rng: R) -> Self {
        let num_bytes = rng.gen_range(DATA_LEN_LOWER_BOUND..=DATA_LEN_UPPER_BOUND);
        Self {
            addr_idx,
            num_bytes,
        }
    }

    #[cfg(any(doc, feature = "rand"))]
    pub(crate) fn new(addr_idx: usize) -> Self {
        Self::from_rng(addr_idx, rand::thread_rng())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AutoNATv2Codec {
    observed_addr: Multiaddr,
    connection_id: ConnectionId,
}

#[async_trait::async_trait]
impl Codec for AutoNATv2Codec {
    type Protocol = String;
    type Request = Request;
    type Response = Response;

    async fn read_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let bytes = read_length_prefixed(io, REQUEST_MAX_SIZE).await?;
        Request::from_bytes(&bytes)
    }

    async fn read_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let bytes = read_length_prefixed(io, 1024).await?;
        Response::from_bytes(&bytes)
    }

    async fn write_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_length_prefixed(io, req.into_bytes()).await?;
        io.close().await
    }

    async fn write_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
        res: Self::Response,
    ) -> std::io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_length_prefixed(io, res.into_bytes()).await?;
        io.close().await
    }
}

#[cfg(test)]
mod tests {
    use crate::generated::structs::{mod_Message::OneOfmsg, DialDataResponse, Message};

    #[test]
    fn message_correct_max_size() {
        let message_bytes = quick_protobuf::serialize_into_vec(&Message {
            msg: OneOfmsg::dialDataResponse(DialDataResponse {
                data: Some(vec![0; 4096].into()),
            }),
        })
        .unwrap();
        assert_eq!(message_bytes.len(), super::REQUEST_MAX_SIZE);
    }
}
