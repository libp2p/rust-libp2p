// change to quick-protobuf-codec

use std::io::ErrorKind;
use std::{borrow::Cow, io};

use asynchronous_codec::{Framed, FramedRead, FramedWrite};

use futures::{AsyncRead, AsyncWrite, AsyncWriteExt, SinkExt, StreamExt};
use libp2p_core::Multiaddr;

use quick_protobuf_codec::Codec;
use rand::Rng;

use crate::{generated::structs as proto, Nonce};

const REQUEST_MAX_SIZE: usize = 4104;
pub(super) const DATA_LEN_LOWER_BOUND: usize = 30_000u32 as usize;
pub(super) const DATA_LEN_UPPER_BOUND: usize = 100_000u32 as usize;
pub(super) const DATA_FIELD_LEN_UPPER_BOUND: usize = 4096;

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

pub(crate) struct Coder<I> {
    inner: Framed<I, Codec<proto::Message>>,
}

impl<I> Coder<I>
where
    I: AsyncWrite + AsyncRead + Unpin,
{
    pub(crate) fn new(io: I) -> Self {
        Self {
            inner: Framed::new(io, Codec::new(REQUEST_MAX_SIZE)),
        }
    }
    pub(crate) async fn close(self) -> io::Result<()> {
        let mut stream = self.inner.into_inner();
        stream.close().await?;
        Ok(())
    }
}

impl<I> Coder<I>
where
    I: AsyncRead + Unpin,
{
    async fn next_msg(&mut self) -> io::Result<proto::Message> {
        self.inner
            .next()
            .await
            .ok_or(io::Error::new(ErrorKind::Other, "no request to read"))?
            .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))
    }
    pub(crate) async fn next_request(&mut self) -> io::Result<Request> {
        Request::from_proto(self.next_msg().await?)
    }
    pub(crate) async fn next_response(&mut self) -> io::Result<Response> {
        Response::from_proto(self.next_msg().await?)
    }
}

impl<I> Coder<I>
where
    I: AsyncWrite + Unpin,
{
    async fn send_msg(&mut self, msg: proto::Message) -> io::Result<()> {
        self.inner.send(msg).await?;
        Ok(())
    }

    pub(crate) async fn send_request(&mut self, request: Request) -> io::Result<()> {
        self.send_msg(request.into_proto()).await
    }

    pub(crate) async fn send_response(&mut self, response: Response) -> io::Result<()> {
        self.send_msg(response.into_proto()).await
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Request {
    Dial(DialRequest),
    Data(DialDataResponse),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DialRequest {
    pub(crate) addrs: Vec<Multiaddr>,
    pub(crate) nonce: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DialDataResponse {
    pub(crate) data_count: usize,
}

impl Request {
    pub(crate) fn from_proto(msg: proto::Message) -> io::Result<Self> {
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

    pub(crate) fn into_proto(self) -> proto::Message {
        match self {
            Request::Dial(DialRequest { addrs, nonce }) => {
                let addrs = addrs.iter().map(|e| e.to_vec()).collect();
                let nonce = Some(nonce);
                proto::Message {
                    msg: proto::mod_Message::OneOfmsg::dialRequest(proto::DialRequest {
                        addrs,
                        nonce,
                    }),
                }
            }
            Request::Data(DialDataResponse { data_count }) => {
                debug_assert!(
                    data_count <= DATA_FIELD_LEN_UPPER_BOUND,
                    "data_count too large"
                );
                static DATA: &[u8] = &[0u8; DATA_FIELD_LEN_UPPER_BOUND];
                proto::Message {
                    msg: proto::mod_Message::OneOfmsg::dialDataResponse(proto::DialDataResponse {
                        data: Some(Cow::Borrowed(&DATA[..data_count])),
                    }),
                }
            }
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
    pub(crate) fn from_proto(msg: proto::Message) -> std::io::Result<Self> {
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

    pub(crate) fn into_proto(self) -> proto::Message {
        match self {
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
        }
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
}

const DIAL_BACK_MAX_SIZE: usize = 10;

pub(crate) struct DialBack {
    pub(crate) nonce: Nonce,
}

impl DialBack {
    pub(crate) async fn read_from(io: impl AsyncRead + Unpin) -> io::Result<Self> {
        let proto::DialBack { nonce } =
            FramedRead::new(io, Codec::<proto::DialBack>::new(DIAL_BACK_MAX_SIZE))
                .next()
                .await
                .ok_or(io::Error::new(io::ErrorKind::UnexpectedEof, "eof"))??;
        let nonce = check_existence!(nonce)?;
        Ok(Self { nonce })
    }

    pub(crate) async fn write_into(self, io: impl AsyncWrite + Unpin) -> io::Result<()> {
        let msg = proto::DialBack {
            nonce: Some(self.nonce),
        };
        FramedWrite::new(io, Codec::<proto::DialBack>::new(DIAL_BACK_MAX_SIZE))
            .send(msg)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}

#[cfg(test)]
mod tests {
    use crate::generated::structs::{
        mod_Message::OneOfmsg, DialDataResponse as GenDialDataResponse, Message,
    };
    use crate::request_response::{Coder, DialDataResponse, Request};
    use futures::io::Cursor;

    use rand::{thread_rng, Rng};

    #[test]
    fn message_correct_max_size() {
        let message_bytes = quick_protobuf::serialize_into_vec(&Message {
            msg: OneOfmsg::dialDataResponse(GenDialDataResponse {
                data: Some(vec![0; 4096].into()),
            }),
        })
        .unwrap();
        assert_eq!(message_bytes.len(), super::REQUEST_MAX_SIZE);
    }

    #[test]
    fn dial_back_correct_size() {
        let dial_back = super::proto::DialBack { nonce: Some(0) };
        let buf = quick_protobuf::serialize_into_vec(&dial_back).unwrap();
        assert!(buf.len() <= super::DIAL_BACK_MAX_SIZE);

        let dial_back_none = super::proto::DialBack { nonce: None };
        let buf = quick_protobuf::serialize_into_vec(&dial_back_none).unwrap();
        assert!(buf.len() <= super::DIAL_BACK_MAX_SIZE);

        let dial_back_max_nonce = super::proto::DialBack {
            nonce: Some(u64::MAX),
        };
        let buf = quick_protobuf::serialize_into_vec(&dial_back_max_nonce).unwrap();
        assert!(buf.len() <= super::DIAL_BACK_MAX_SIZE);
    }

    #[tokio::test]
    async fn write_read_request() {
        let mut buf = Cursor::new(Vec::new());
        let mut coder = Coder::new(&mut buf);
        let mut all_req = Vec::with_capacity(100);
        for _ in 0..100 {
            let data_request: Request = Request::Data(DialDataResponse {
                data_count: thread_rng().gen_range(0..4000),
            });
            all_req.push(data_request.clone());
            coder.send_request(data_request.clone()).await.unwrap();
        }
        let inner = coder.inner.into_inner();
        inner.set_position(0);
        let mut coder = Coder::new(inner);
        for i in 0..100 {
            let read_data_request = coder.next_request().await.unwrap();
            assert_eq!(read_data_request, all_req[i]);
        }
    }
}
