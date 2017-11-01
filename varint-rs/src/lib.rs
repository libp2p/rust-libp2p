extern crate num_bigint;
extern crate tokio_io;
extern crate bytes;
extern crate futures;

use bytes::BytesMut;
use num_bigint::BigUint;
use tokio_io::AsyncRead;
use tokio_io::codec::Decoder;
use std::io;
use std::io::prelude::*;

// TODO: error-chain
pub struct ParseError;

#[derive(Default)]
pub struct DecoderState {
    // TODO: Non-allocating `BigUint`?
    accumulator: BigUint,
    shift: usize,
}

impl DecoderState {
    pub fn new() -> Self {
        Default::default()
    }

    fn decode_one(mut self, byte: u8) -> Result<BigUint, Self> {
        self.accumulator = self.accumulator | (BigUint::from(byte & 0x7F) << self.shift);
        self.shift += 7;

        if byte & 0x80 == 0 {
            Ok(self.accumulator)
        } else {
            Err(self)
        }
    }

    // Why the weird type signature? Well, `BigUint` owns its storage, and we don't want to clone
    // it. So, we want the accumulator to be moved out when it is ready. We could have also used
    // `Option`, but this means that it's not possible to end up in an inconsistent state
    // (`shift != 0 && accumulator.is_none()`).
    pub fn read<R: AsyncRead>(self, mut input: R) -> Result<Result<BigUint, Self>, ParseError> {
        let mut state = self;
        loop {
            // We read one at a time to prevent consuming too much of the buffer.
            let mut buffer: [u8; 1] = [0];

            match input.read_exact(&mut buffer) {
                Ok(_) => {
                    state = match state.decode_one(buffer[0]) {
                        Ok(out) => break Ok(Ok(out)),
                        Err(state) => state,
                    };
                }
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => break Ok(Err(state)),
                Err(_) => break Err(ParseError),
            }
        }
    }
}

#[derive(Default)]
pub struct VarintDecoder {
    state: Option<DecoderState>,
}

impl VarintDecoder {
    pub fn new() -> Self {
        Default::default()
    }
}

impl Decoder for VarintDecoder {
    type Item = BigUint;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            if src.len() == 0 {
                break Err(io::Error::from(io::ErrorKind::UnexpectedEof));
            } else {
                // We know that the length is not 0, so this cannot fail.
                let first_byte = src.split_to(1)[0];
                let new_state = match self.state.take() {
                    Some(state) => state.decode_one(first_byte),
                    None => DecoderState::new().decode_one(first_byte),
                };

                match new_state {
                    Ok(out) => break Ok(Some(out)),
                    Err(state) => self.state = Some(state),
                }
            }
        }
    }
}

pub fn decode<R: Read>(stream: R) -> io::Result<BigUint> {
    let mut out = BigUint::from(0u8);
    let mut shift = 0;
    let mut finished_cleanly = false;

    for i in stream.bytes() {
        let i = i?;

        out = out | (BigUint::from(i & 0x7F) << shift);
        shift += 7;

        if i & 0x80 == 0 {
            finished_cleanly = true;
            break;
        }
    }

    if finished_cleanly {
        Ok(out)
    } else {
        Err(io::Error::from(io::ErrorKind::UnexpectedEof))
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, VarintDecoder};
    use tokio_io::codec::FramedRead;
    use num_bigint::BigUint;
    use futures::{Future, Stream};

    #[test]
    fn can_decode_basic_uint() {
        assert_eq!(
            BigUint::from(300u16),
            decode(&[0b10101100, 0b00000010][..]).unwrap()
        );
    }

    #[test]
    fn can_decode_basic_uint_async() {
        let result = FramedRead::new(&[0b10101100, 0b00000010][..], VarintDecoder::new())
            .into_future()
            .map(|(out, _)| out)
            .map_err(|(out, _)| out)
            .wait();

        assert_eq!(result.unwrap(), Some(BigUint::from(300u16)));
    }

    #[test]
    fn unexpected_eof_async() {
        use std::io;

        let result = FramedRead::new(&[0b10101100, 0b10000010][..], VarintDecoder::new())
            .into_future()
            .map(|(out, _)| out)
            .map_err(|(out, _)| out)
            .wait();

        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }
}
