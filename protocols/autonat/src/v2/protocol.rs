// change to quick-protobuf-codec

use std::{io, io::ErrorKind};

use asynchronous_codec::{Framed, FramedRead, FramedWrite};
use futures::{AsyncRead, AsyncWrite, SinkExt, StreamExt};
use libp2p_core::Multiaddr;
use quick_protobuf_codec::Codec;
use rand::Rng;

use crate::v2::{generated::structs as proto, Nonce};

const REQUEST_MAX_SIZE: usize = 4104;
pub(super) const DATA_LEN_LOWER_BOUND: usize = 30_000u32 as usize;
pub(super) const DATA_LEN_UPPER_BOUND: usize = 100_000u32 as usize;
pub(super) const DATA_FIELD_LEN_UPPER_BOUND: usize = 4096;

fn new_io_invalid_data_err(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
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
    pub(crate) async fn close(mut self) -> io::Result<()> {
        self.inner.close().await?;
        Ok(())
    }
}

impl<I> Coder<I>
where
    I: AsyncRead + Unpin,
{
    pub(crate) async fn next<M, E>(&mut self) -> io::Result<M>
    where
        proto::Message: TryInto<M, Error = E>,
        io::Error: From<E>,
    {
        Ok(self.next_msg().await?.try_into()?)
    }

    async fn next_msg(&mut self) -> io::Result<proto::Message> {
        self.inner
            .next()
            .await
            .ok_or(io::Error::new(
                ErrorKind::UnexpectedEof,
                "no request to read",
            ))?
            .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))
    }
}

impl<I> Coder<I>
where
    I: AsyncWrite + Unpin,
{
    pub(crate) async fn send<M>(&mut self, msg: M) -> io::Result<()>
    where
        M: Into<proto::Message>,
    {
        self.inner.send(msg.into()).await?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Request {
    Dial(DialRequest),
    Data(DialDataResponse),
}

impl From<DialRequest> for proto::Message {
    fn from(val: DialRequest) -> Self {
        let addrs = val.addrs.iter().map(|e| e.to_vec()).collect();
        let nonce = val.nonce;

        proto::Message {
            msg: proto::mod_Message::OneOfmsg::dialRequest(proto::DialRequest { addrs, nonce }),
        }
    }
}

impl From<DialDataResponse> for proto::Message {
    fn from(val: DialDataResponse) -> Self {
        debug_assert!(
            val.data_count <= DATA_FIELD_LEN_UPPER_BOUND,
            "data_count too large"
        );
        proto::Message {
            msg: proto::mod_Message::OneOfmsg::dialDataResponse(proto::DialDataResponse {
                // One could use Cow::Borrowed here, but it will
                // require a modification of the generated code
                // and that will fail the CI
                data: vec![0; val.data_count],
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DialRequest {
    pub(crate) addrs: Vec<Multiaddr>,
    pub(crate) nonce: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DialDataResponse {
    data_count: usize,
}

impl DialDataResponse {
    pub(crate) fn new(data_count: usize) -> Option<Self> {
        if data_count <= DATA_FIELD_LEN_UPPER_BOUND {
            Some(Self { data_count })
        } else {
            None
        }
    }

    pub(crate) fn get_data_count(&self) -> usize {
        self.data_count
    }
}

impl TryFrom<proto::Message> for Request {
    type Error = io::Error;

    fn try_from(msg: proto::Message) -> Result<Self, Self::Error> {
        match msg.msg {
            proto::mod_Message::OneOfmsg::dialRequest(proto::DialRequest { addrs, nonce }) => {
                let addrs = addrs
                    .into_iter()
                    .map(|e| e.to_vec())
                    .map(|e| {
                        Multiaddr::try_from(e).map_err(|err| {
                            new_io_invalid_data_err(format!("invalid multiaddr: {}", err))
                        })
                    })
                    .collect::<Result<Vec<_>, io::Error>>()?;
                Ok(Self::Dial(DialRequest { addrs, nonce }))
            }
            proto::mod_Message::OneOfmsg::dialDataResponse(proto::DialDataResponse { data }) => {
                let data_count = data.len();
                Ok(Self::Data(DialDataResponse { data_count }))
            }
            _ => Err(new_io_invalid_data_err(
                "expected dialResponse or dialDataRequest",
            )),
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

impl TryFrom<proto::Message> for Response {
    type Error = io::Error;

    fn try_from(msg: proto::Message) -> Result<Self, Self::Error> {
        match msg.msg {
            proto::mod_Message::OneOfmsg::dialResponse(proto::DialResponse {
                status,
                addrIdx,
                dialStatus,
            }) => Ok(Response::Dial(DialResponse {
                status,
                addr_idx: addrIdx as usize,
                dial_status: dialStatus,
            })),
            proto::mod_Message::OneOfmsg::dialDataRequest(proto::DialDataRequest {
                addrIdx,
                numBytes,
            }) => Ok(Self::Data(DialDataRequest {
                addr_idx: addrIdx as usize,
                num_bytes: numBytes as usize,
            })),
            _ => Err(new_io_invalid_data_err(
                "invalid message type, expected dialResponse or dialDataRequest",
            )),
        }
    }
}

impl From<Response> for proto::Message {
    fn from(val: Response) -> Self {
        match val {
            Response::Dial(DialResponse {
                status,
                addr_idx,
                dial_status,
            }) => proto::Message {
                msg: proto::mod_Message::OneOfmsg::dialResponse(proto::DialResponse {
                    status,
                    addrIdx: addr_idx as u32,
                    dialStatus: dial_status,
                }),
            },
            Response::Data(DialDataRequest {
                addr_idx,
                num_bytes,
            }) => proto::Message {
                msg: proto::mod_Message::OneOfmsg::dialDataRequest(proto::DialDataRequest {
                    addrIdx: addr_idx as u32,
                    numBytes: num_bytes as u64,
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

pub(crate) async fn dial_back(stream: impl AsyncWrite + Unpin, nonce: Nonce) -> io::Result<()> {
    let msg = proto::DialBack { nonce };
    let mut framed = FramedWrite::new(stream, Codec::<proto::DialBack>::new(DIAL_BACK_MAX_SIZE));

    framed
        .send(msg)
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    Ok(())
}

pub(crate) async fn recv_dial_back(stream: impl AsyncRead + Unpin) -> io::Result<Nonce> {
    let framed = &mut FramedRead::new(stream, Codec::<proto::DialBack>::new(DIAL_BACK_MAX_SIZE));
    let proto::DialBack { nonce } = framed
        .next()
        .await
        .ok_or(io::Error::from(io::ErrorKind::UnexpectedEof))??;
    Ok(nonce)
}

pub(crate) async fn dial_back_response(stream: impl AsyncWrite + Unpin) -> io::Result<()> {
    let msg = proto::DialBackResponse {
        status: proto::mod_DialBackResponse::DialBackStatus::OK,
    };
    let mut framed = FramedWrite::new(
        stream,
        Codec::<proto::DialBackResponse>::new(DIAL_BACK_MAX_SIZE),
    );
    framed
        .send(msg)
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    Ok(())
}

pub(crate) async fn recv_dial_back_response(
    stream: impl AsyncRead + AsyncWrite + Unpin,
) -> io::Result<()> {
    let framed = &mut FramedRead::new(
        stream,
        Codec::<proto::DialBackResponse>::new(DIAL_BACK_MAX_SIZE),
    );
    let proto::DialBackResponse { status } = framed
        .next()
        .await
        .ok_or(io::Error::from(io::ErrorKind::UnexpectedEof))??;

    if proto::mod_DialBackResponse::DialBackStatus::OK == status {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid dial back response",
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::v2::generated::structs::{
        mod_Message::OneOfmsg, DialDataResponse as GenDialDataResponse, Message,
    };

    #[test]
    fn message_correct_max_size() {
        let message_bytes = quick_protobuf::serialize_into_vec(&Message {
            msg: OneOfmsg::dialDataResponse(GenDialDataResponse {
                data: vec![0; 4096],
            }),
        })
        .unwrap();
        assert_eq!(message_bytes.len(), super::REQUEST_MAX_SIZE);
    }

    #[test]
    fn dial_back_correct_size() {
        let dial_back = super::proto::DialBack { nonce: 0 };
        let buf = quick_protobuf::serialize_into_vec(&dial_back).unwrap();
        assert!(buf.len() <= super::DIAL_BACK_MAX_SIZE);

        let dial_back_max_nonce = super::proto::DialBack { nonce: u64::MAX };
        let buf = quick_protobuf::serialize_into_vec(&dial_back_max_nonce).unwrap();
        assert!(buf.len() <= super::DIAL_BACK_MAX_SIZE);
    }
}
