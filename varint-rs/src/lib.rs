extern crate num_bigint;
extern crate futures;

use std::io;
use std::io::prelude::*;

use futures::{Async, Poll};
use futures::stream::Stream;
use num_bigint::BigUint;

struct DecoderState {
    // TODO: Non-allocating `BigUint`?
    accumulator: BigUint,
    shift: usize,
}

impl DecoderState {
    fn new() -> Self {
        DecoderState {
            accumulator: BigUint::from(0u8),
            shift: 0,
        }
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
}

pub struct Decoder<T> {
    state: Option<DecoderState>,
    inner: T,
}

impl<T: Stream<Item = u8>> From<T> for Decoder<T> {
    fn from(inner: T) -> Self {
        Self::new(inner)
    }
}

impl<T: Stream<Item = u8>> Decoder<T> {
    pub fn new(inner: T) -> Self {
        Decoder { state: None, inner }
    }
}

impl<E, T: Stream<Item = u8, Error = E>> Stream for Decoder<T>
where
    io::Error: From<E>,
{
    type Item = BigUint;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let update = match self.inner.poll()? {
            Async::Ready(byte) => {
                match (self.state.take(), byte) {
                    (Some(state), Some(byte)) => state.decode_one(byte),
                    (None, Some(byte)) => DecoderState::new().decode_one(byte),
                    (None, None) => return Ok(Async::Ready(None)),
                    // `io::Error::from(..)` is broken by the `From` bound above
                    _ => return Err(io::ErrorKind::UnexpectedEof.into()),
                }
            }
            Async::NotReady => return Ok(Async::NotReady),
        };

        match update {
            Ok(out) => Ok(Async::Ready(Some(out))),
            Err(state) => {
                self.state = Some(state);
                Ok(Async::NotReady)
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
    use super::decode;
    use num_bigint::BigUint;

    #[test]
    fn can_decode_basic_uint() {
        assert_eq!(
            BigUint::from(300u16),
            decode(&[0b10101100, 0b00000010][..]).unwrap()
        );
    }
}
