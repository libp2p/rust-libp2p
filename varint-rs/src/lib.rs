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

#![warn(missing_docs)]

//! Encoding and decoding state machines for protobuf varints

// TODO: Non-allocating `BigUint`?
extern crate num_bigint;
extern crate num_traits;
extern crate tokio_io;
extern crate bytes;
extern crate futures;
#[macro_use]
extern crate error_chain;

use bytes::{BufMut, BytesMut, IntoBuf};
use futures::{Poll, Async};
use num_bigint::BigUint;
use num_traits::ToPrimitive;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::{Encoder, Decoder};
use std::io;
use std::io::prelude::*;
use std::marker::PhantomData;
use std::mem;

mod errors {
    error_chain! {
        errors {
            ParseError {
                description("error parsing varint")
                display("error parsing varint")
            }
            WriteError {
                description("error writing varint")
                display("error writing varint")
            }
        }

        foreign_links {
            Io(::std::io::Error);
        }
    }
}

pub use errors::{Error, ErrorKind};

const USABLE_BITS_PER_BYTE: usize = 7;

/// The state struct for the varint-to-bytes FSM
#[derive(Debug)]
pub struct EncoderState<T> {
    source: T,
    // A "chunk" is a section of the `source` `USABLE_BITS_PER_BYTE` bits long
    num_chunks: usize,
    cur_chunk: usize,
}

/// Whether or not the varint writing was completed
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum WriteState {
    /// The encoder has finished writing
    Done(usize),
    /// The encoder still must write more bytes
    Pending(usize),
}

fn ceil_div(a: usize, b: usize) -> usize {
    (a + b - 1) / b
}

/// A trait to get the minimum number of bits required to represent a number
pub trait Bits {
    /// The minimum number of bits required to represent `self`
    fn bits(&self) -> usize;
}

impl Bits for BigUint {
    fn bits(&self) -> usize {
        BigUint::bits(self)
    }
}

macro_rules! impl_bits {
    ($t:ty) => {
        impl Bits for $t {
            fn bits(&self) -> usize {
                (std::mem::size_of::<$t>() * 8) - self.leading_zeros() as usize
            }
        }
    }
}

impl_bits!(usize);
impl_bits!(u64);
impl_bits!(u32);
impl_bits!(u16);
impl_bits!(u8);

/// Helper trait to allow multiple integer types to be encoded
pub trait EncoderHelper: Sized {
    /// Write as much as possible of the inner integer to the output `AsyncWrite`
    fn write<W: AsyncWrite>(encoder: &mut EncoderState<Self>, output: W)
        -> Poll<WriteState, Error>;
}

/// Helper trait to allow multiple integer types to be encoded
pub trait DecoderHelper: Sized {
    /// Decode a single byte
    fn decode_one(decoder: &mut DecoderState<Self>, byte: u8) -> errors::Result<Option<Self>>;

    /// Read as much of the varint as possible
    fn read<R: AsyncRead>(decoder: &mut DecoderState<Self>, input: R) -> Poll<Option<Self>, Error>;
}

macro_rules! impl_decoderstate {
    ($t:ty) => {
        impl_decoderstate!(
            $t,
            |a| a as $t,
            |a: $t, b| -> Option<$t> { a.checked_shl(b as u32) }
        );
    };
    ($t:ty, $make_fn:expr) => { impl_decoderstate!($t, $make_fn, $make_fn); };
    ($t:ty, $make_fn:expr, $shift_fn:expr) => {
        impl DecoderHelper for $t {
            #[inline]
            fn decode_one(decoder: &mut DecoderState<Self>, byte: u8) -> ::errors::Result<Option<$t>> {
                let res = decoder.accumulator.take().and_then(|accumulator| {
                    let out = accumulator | match $shift_fn(
                        $make_fn(byte & 0x7F),
                        decoder.shift * USABLE_BITS_PER_BYTE,
                    ) {
                        Some(a) => a,
                        None => return Some(Err(ErrorKind::ParseError.into())),
                    };
                    decoder.shift += 1;

                    if byte & 0x80 == 0 {
                        Some(Ok(out))
                    } else {
                        decoder.accumulator = AccumulatorState::InProgress(out);
                        None
                    }
                });

                match res {
                    Some(Ok(number)) => Ok(Some(number)),
                    Some(Err(err)) => Err(err),
                    None => Ok(None),
                }
            }

            fn read<R: AsyncRead>(
                decoder: &mut DecoderState<Self>,
                mut input: R
            ) -> Poll<Option<Self>, Error> {
                if decoder.accumulator == AccumulatorState::Finished {
                    return Err(Error::with_chain(
                        io::Error::new(
                            io::ErrorKind::Other,
                            "Attempted to parse a second varint (create a new instance!)",
                        ),
                        ErrorKind::ParseError,
                    ));
                }

                loop {
                    // We read one at a time to prevent consuming too much of the buffer.
                    let mut buffer: [u8; 1] = [0];

                    match input.read_exact(&mut buffer) {
                        Ok(()) => {
                            if let Some(out) = Self::decode_one(decoder, buffer[0])? {
                                break Ok(Async::Ready(Some(out)));
                            }
                        }
                        Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                            break Ok(Async::NotReady);
                        }
                        Err(inner) => if decoder.accumulator == AccumulatorState::NotStarted {
                            break Ok(Async::Ready(None));
                        } else {
                            break Err(Error::with_chain(inner, ErrorKind::ParseError))
                        },
                    }
                }
            }
        }
    }
}

macro_rules! impl_encoderstate {
    ($t:ty) => { impl_encoderstate!($t, <$t>::from); };
    ($t:ty, $make_fn:expr) => {
        impl EncoderHelper for $t {
            /// Write as much as possible of the inner integer to the output `AsyncWrite`
            fn write<W: AsyncWrite>(
                encoder: &mut EncoderState<Self>,
                mut output: W,
            ) -> Poll<WriteState, Error> {
                fn encode_one(encoder: &EncoderState<$t>) -> Option<u8> {
                    let last_chunk = encoder.num_chunks - 1;

                    if encoder.cur_chunk > last_chunk {
                        return None;
                    }

                    let masked = (&encoder.source >> (encoder.cur_chunk * USABLE_BITS_PER_BYTE)) &
                        $make_fn((1 << USABLE_BITS_PER_BYTE) - 1usize);
                    let masked = masked.to_u8().expect(
                        "Masked with 0b0111_1111, is less than u8::MAX, QED",
                    );

                    if encoder.cur_chunk == last_chunk {
                        Some(masked)
                    } else {
                        Some(masked | (1 << USABLE_BITS_PER_BYTE))
                    }
                }

                let mut written = 0usize;

                loop {
                    if let Some(byte) = encode_one(&encoder) {
                        let buffer: [u8; 1] = [byte];

                        match output.write_all(&buffer) {
                            Ok(()) => {
                                written += 1;
                                encoder.cur_chunk += 1;
                            }
                            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                                break if written == 0 {
                                    Ok(Async::NotReady)
                                } else {
                                    Ok(Async::Ready(WriteState::Pending(written)))
                                };
                            }
                            Err(inner) => break Err(
                                Error::with_chain(inner, ErrorKind::WriteError)
                            ),
                        }
                    } else {
                        break Ok(Async::Ready(WriteState::Done(written)));
                    }
                }
            }
        }
    }
}

impl_encoderstate!(usize);
impl_encoderstate!(BigUint);
impl_encoderstate!(u64, (|val| val as u64));
impl_encoderstate!(u32, (|val| val as u32));

impl_decoderstate!(usize);
impl_decoderstate!(BigUint, BigUint::from, |a, b| Some(a << b));
impl_decoderstate!(u64);
impl_decoderstate!(u32);

impl<T> EncoderState<T> {
    pub fn source(&self) -> &T {
        &self.source
    }
}

impl<T: Bits> EncoderState<T> {
    /// Create a new encoder
    pub fn new(inner: T) -> Self {
        let bits = inner.bits();
        EncoderState {
            source: inner,
            num_chunks: ceil_div(bits, USABLE_BITS_PER_BYTE).max(1),
            cur_chunk: 0,
        }
    }
}

impl<T: EncoderHelper> EncoderState<T> {
    /// Write as much as possible of the inner integer to the output `AsyncWrite`
    pub fn write<W: AsyncWrite>(&mut self, output: W) -> Poll<WriteState, Error> {
        T::write(self, output)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AccumulatorState<T> {
    InProgress(T),
    NotStarted,
    Finished,
}

impl<T: Default> AccumulatorState<T> {
    fn take(&mut self) -> Option<T> {
        use std::mem;
        use AccumulatorState::*;

        match mem::replace(self, AccumulatorState::Finished) {
            InProgress(inner) => Some(inner),
            NotStarted => Some(Default::default()),
            Finished => None,
        }
    }
}

/// The state struct for the varint bytes-to-bigint FSM
#[derive(Debug)]
pub struct DecoderState<T> {
    accumulator: AccumulatorState<T>,
    shift: usize,
}

impl<T: Default> Default for DecoderState<T> {
    fn default() -> Self {
        DecoderState {
            accumulator: AccumulatorState::NotStarted,
            shift: 0,
        }
    }
}

impl<T: Default> DecoderState<T> {
    /// Make a new decoder
    pub fn new() -> Self {
        Default::default()
    }
}

impl<T: DecoderHelper> DecoderState<T> {
    /// Make a new decoder
    pub fn read<R: AsyncRead>(&mut self, input: R) -> Poll<Option<T>, Error> {
        T::read(self, input)
    }
}

/// Wrapper around `DecoderState` to make a `tokio` `Decoder`
#[derive(Debug)]
pub struct VarintDecoder<T> {
    state: Option<DecoderState<T>>,
}

impl<T> Default for VarintDecoder<T> {
    fn default() -> Self {
        VarintDecoder { state: None }
    }
}

impl<T> VarintDecoder<T> {
    /// Make a new decoder
    pub fn new() -> Self {
        Default::default()
    }
}

impl<T: Default + DecoderHelper> Decoder for VarintDecoder<T> {
    type Item = T;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            if src.is_empty() && self.state.is_some() {
                break Err(io::Error::from(io::ErrorKind::UnexpectedEof));
            } else {
                // We know that the length is not 0, so this cannot fail.
                let first_byte = src.split_to(1)[0];
                let mut state = self.state.take().unwrap_or_default();
                let out = T::decode_one(&mut state, first_byte)
                    .map_err(|_| io::Error::from(io::ErrorKind::Other))?;

                if let Some(out) = out {
                    break Ok(Some(out));
                } else {
                    self.state = Some(state);
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct VarintCodec<W> {
    inner: VarintCodecInner,
    marker: PhantomData<W>,
}

impl<T> Default for VarintCodec<T> {
    #[inline]
    fn default() -> VarintCodec<T> {
        VarintCodec {
            inner: VarintCodecInner::WaitingForLen(VarintDecoder::default()),
            marker: PhantomData,
        }
    }
}

#[derive(Debug)]
enum VarintCodecInner {
    WaitingForLen(VarintDecoder<usize>),
    WaitingForData(usize),
    Poisonned,
}

impl<T> Decoder for VarintCodec<T> {
    type Item = BytesMut;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            match mem::replace(&mut self.inner, VarintCodecInner::Poisonned) {
                VarintCodecInner::WaitingForData(len) => {
                    if src.len() >= len {
                        self.inner = VarintCodecInner::WaitingForLen(VarintDecoder::default());
                        return Ok(Some(src.split_to(len)));
                    } else {
                        self.inner = VarintCodecInner::WaitingForData(len);
                        return Ok(None);
                    }
                },
                VarintCodecInner::WaitingForLen(mut decoder) => {
                    match decoder.decode(src)? {
                        None => {
                            self.inner = VarintCodecInner::WaitingForLen(decoder);
                            return Ok(None);
                        },
                        Some(len) => {
                            self.inner = VarintCodecInner::WaitingForData(len);
                        },
                    }
                },
                VarintCodecInner::Poisonned => {
                    panic!("varint codec was poisoned")
                },
            }
        }
    }
}

impl<D> Encoder for VarintCodec<D>
    where D: IntoBuf + AsRef<[u8]>,
{
    type Item = D;
    type Error = io::Error;

    fn encode(&mut self, item: D, dst: &mut BytesMut) -> Result<(), io::Error> {
        let encoded_len = encode(item.as_ref().len());       // TODO: can be optimized by not allocating?
        dst.put(encoded_len);
        dst.put(item);
        Ok(())
    }
}

/// Syncronously decode a number from a `Read`
pub fn decode<R: Read, T: Default + DecoderHelper>(mut input: R) -> errors::Result<T> {
    let mut decoder = DecoderState::default();

    loop {
        // We read one at a time to prevent consuming too much of the buffer.
        let mut buffer: [u8; 1] = [0];

        match input.read_exact(&mut buffer) {
            Ok(()) => {
                if let Some(out) = T::decode_one(&mut decoder, buffer[0])
                    .map_err(|_| io::Error::from(io::ErrorKind::Other))?
                {
                    break Ok(out);
                }
            }
            Err(inner) => break Err(Error::with_chain(inner, ErrorKind::ParseError)),
        }
    }
}

/// Syncronously decode a number from a `Read`
pub fn encode<T: EncoderHelper + Bits>(input: T) -> Vec<u8> {
    let mut encoder = EncoderState::new(input);
    let mut out = io::Cursor::new(Vec::with_capacity(1));

    match T::write(&mut encoder, &mut out).expect("Writing to a vec should never fail, Q.E.D") {
        Async::Ready(_) => out.into_inner(),
        Async::NotReady => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, VarintDecoder, EncoderState};
    use tokio_io::codec::FramedRead;
    use num_bigint::BigUint;
    use futures::{Future, Stream};

    #[test]
    fn large_number_fails() {
        use std::io::Cursor;
        use futures::Async;
        use super::WriteState;

        let mut out = vec![0u8; 10];

        {
            let writable: Cursor<&mut [_]> = Cursor::new(&mut out);

            let mut state = EncoderState::new(::std::u64::MAX);

            assert_eq!(
                state.write(writable).unwrap(),
                Async::Ready(WriteState::Done(10))
            );
        }

        let result: Result<Option<u32>, _> = FramedRead::new(&out[..], VarintDecoder::new())
            .into_future()
            .map(|(out, _)| out)
            .map_err(|(out, _)| out)
            .wait();

        assert!(result.is_err());
    }

    #[test]
    fn can_decode_basic_biguint() {
        assert_eq!(
            BigUint::from(300u16),
            decode(&[0b10101100, 0b00000010][..]).unwrap()
        );
    }

    #[test]
    fn can_decode_basic_biguint_async() {
        let result = FramedRead::new(&[0b10101100, 0b00000010][..], VarintDecoder::new())
            .into_future()
            .map(|(out, _)| out)
            .map_err(|(out, _)| out)
            .wait();

        assert_eq!(result.unwrap(), Some(BigUint::from(300u16)));
    }

    #[test]
    fn can_decode_trivial_usize_async() {
        let result = FramedRead::new(&[1][..], VarintDecoder::new())
            .into_future()
            .map(|(out, _)| out)
            .map_err(|(out, _)| out)
            .wait();

        assert_eq!(result.unwrap(), Some(1usize));
    }

    #[test]
    fn can_decode_basic_usize_async() {
        let result = FramedRead::new(&[0b10101100, 0b00000010][..], VarintDecoder::new())
            .into_future()
            .map(|(out, _)| out)
            .map_err(|(out, _)| out)
            .wait();

        assert_eq!(result.unwrap(), Some(300usize));
    }

    #[test]
    fn can_encode_basic_biguint_async() {
        use std::io::Cursor;
        use futures::Async;
        use super::WriteState;

        let mut out = vec![0u8; 2];

        {
            let writable: Cursor<&mut [_]> = Cursor::new(&mut out);

            let mut state = EncoderState::new(BigUint::from(300usize));

            assert_eq!(
                state.write(writable).unwrap(),
                Async::Ready(WriteState::Done(2))
            );
        }

        assert_eq!(out, vec![0b10101100, 0b00000010]);
    }

    #[test]
    fn can_encode_basic_usize_async() {
        use std::io::Cursor;
        use futures::Async;
        use super::WriteState;

        let mut out = vec![0u8; 2];

        {
            let writable: Cursor<&mut [_]> = Cursor::new(&mut out);

            let mut state = EncoderState::new(300usize);

            assert_eq!(
                state.write(writable).unwrap(),
                Async::Ready(WriteState::Done(2))
            );
        }

        assert_eq!(out, vec![0b10101100, 0b00000010]);
    }

    #[test]
    fn unexpected_eof_async() {
        use std::io;

        let result = FramedRead::new(&[0b10101100, 0b10000010][..], VarintDecoder::<usize>::new())
            .into_future()
            .map(|(out, _)| out)
            .map_err(|(out, _)| out)
            .wait();

        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }
}
