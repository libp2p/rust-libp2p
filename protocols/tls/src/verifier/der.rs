// Copyright 2020 Parity Technologies (UK) Ltd.
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

//! A wrapper for [*ring*](https://github.com/briansmith/ring)’s DER parser.
use ring::{error::Unspecified, io::der};
use untrusted::{Input, Reader};
use webpki::Error;
type Result<T> = std::result::Result<T, Error>;
pub(super) use der::Tag;

#[inline]
pub(super) fn expect_tag_and_get_value<'a>(input: &mut Reader<'a>, tag: Tag) -> Result<Input<'a>> {
    der::expect_tag_and_get_value(input, tag).map_err(|Unspecified| Error::BadDER)
}

#[inline]
pub(super) fn bit_string_with_no_unused_bits<'a>(input: &mut Reader<'a>) -> Result<Input<'a>> {
    der::bit_string_with_no_unused_bits(input).map_err(|Unspecified| Error::BadDER)
}

#[inline]
pub(super) fn nested_sequence<'a, T, U>(input: &mut Reader<'a>, decoder: U) -> Result<T>
where
    U: FnMut(&mut Reader<'a>) -> Result<T>,
{
    nested(input, Tag::Sequence, decoder)
}

#[inline]
pub(super) fn nested<'a, T, U>(input: &mut Reader<'a>, tag: Tag, decoder: U) -> Result<T>
where
    U: FnMut(&mut Reader<'a>) -> Result<T>,
{
    der::nested(input, tag, Error::BadDER, decoder)
}

#[inline]
pub(super) fn positive_integer<'a>(input: &mut Reader<'a>) -> Result<ring::io::Positive<'a>> {
    der::positive_integer(input).map_err(|Unspecified| Error::BadDER)
}
