// Copyright 2017 Parity Technologies (UK) Ltd.
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

const FLAG_BITS: usize = 3;
const FLAG_MASK: usize = (1usize << FLAG_BITS) - 1;

pub mod errors {
    error_chain! {
        errors {
            ParseError
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum MultiplexEnd {
    Initiator,
    Receiver,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct MultiplexHeader {
    pub packet_type: PacketType,
    pub substream_id: u32,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PacketType {
    Open,
    Close(MultiplexEnd),
    Reset(MultiplexEnd),
    Message(MultiplexEnd),
}

impl MultiplexHeader {
    pub fn open(id: u32) -> Self {
        MultiplexHeader {
            substream_id: id,
            packet_type: PacketType::Open,
        }
    }

    pub fn close(id: u32, end: MultiplexEnd) -> Self {
        MultiplexHeader {
            substream_id: id,
            packet_type: PacketType::Close(end),
        }
    }

    pub fn reset(id: u32, end: MultiplexEnd) -> Self {
        MultiplexHeader {
            substream_id: id,
            packet_type: PacketType::Reset(end),
        }
    }

    pub fn message(id: u32, end: MultiplexEnd) -> Self {
        MultiplexHeader {
            substream_id: id,
            packet_type: PacketType::Message(end),
        }
    }

    // TODO: Use `u128` or another large integer type instead of bigint since we never use more than
    //       `pointer width + FLAG_BITS` bits and unconditionally allocating 1-3 `u32`s for that is
    //       ridiculous (especially since even for small numbers we have to allocate 1 `u32`).
    //       If this is the future and `BigUint` is better-optimised (maybe by using `Bytes`) then
    //       forget it.
    pub fn parse(header: u64) -> Result<MultiplexHeader, errors::Error> {
        use num_traits::cast::ToPrimitive;

        let flags = header & FLAG_MASK as u64;

        let substream_id = (header >> FLAG_BITS)
            .to_u32()
            .ok_or(errors::ErrorKind::ParseError)?;

        // Yes, this is really how it works. No, I don't know why.
        let packet_type = match flags {
            0 => PacketType::Open,

            1 => PacketType::Message(MultiplexEnd::Receiver),
            2 => PacketType::Message(MultiplexEnd::Initiator),

            3 => PacketType::Close(MultiplexEnd::Receiver),
            4 => PacketType::Close(MultiplexEnd::Initiator),

            5 => PacketType::Reset(MultiplexEnd::Receiver),
            6 => PacketType::Reset(MultiplexEnd::Initiator),

            _ => {
                use std::io;

                return Err(errors::Error::with_chain(
                    io::Error::new(
                        io::ErrorKind::Other,
                        format!("Unexpected packet type: {}", flags),
                    ),
                    errors::ErrorKind::ParseError,
                ));
            }
        };

        Ok(MultiplexHeader {
            substream_id,
            packet_type,
        })
    }

    pub fn to_u64(&self) -> u64 {
        let packet_type_id = match self.packet_type {
            PacketType::Open => 0,

            PacketType::Message(MultiplexEnd::Receiver) => 1,
            PacketType::Message(MultiplexEnd::Initiator) => 2,

            PacketType::Close(MultiplexEnd::Receiver) => 3,
            PacketType::Close(MultiplexEnd::Initiator) => 4,

            PacketType::Reset(MultiplexEnd::Receiver) => 5,
            PacketType::Reset(MultiplexEnd::Initiator) => 6,
        };

        let substream_id = (self.substream_id as u64) << FLAG_BITS;

        substream_id | packet_type_id
    }
}
