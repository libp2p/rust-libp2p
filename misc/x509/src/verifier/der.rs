// Copyright 2020 Parity Technologies (UK) Ltd.
// Copyright 2015 Brian Smith.
//
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

use super::calendar;
pub(super) use ring::io::{
    der::{nested, Tag},
    Positive,
};
use webpki::{Error, Time};

#[inline(always)]
pub(super) fn expect_tag_and_get_value<'a>(
    input: &mut untrusted::Reader<'a>, tag: Tag,
) -> Result<untrusted::Input<'a>, Error> {
    ring::io::der::expect_tag_and_get_value(input, tag).map_err(|_| Error::BadDER)
}

pub(super) fn expect_bytes(
    input: &mut untrusted::Reader<'_>, bytes: &[u8], error: Error,
) -> Result<(), Error> {
    match input.read_bytes(bytes.len()) {
        Ok(e) if e == untrusted::Input::from(bytes) => Ok(()),
        _ => Err(error),
    }
}

#[inline(always)]
pub(super) fn bit_string_with_no_unused_bits<'a>(
    input: &mut untrusted::Reader<'a>,
) -> Result<untrusted::Input<'a>, Error> {
    ring::io::der::bit_string_with_no_unused_bits(input).map_err(|_| Error::BadDER)
}

#[inline(always)]
pub(super) fn positive_integer<'a>(
    input: &'a mut untrusted::Reader<'_>,
) -> Result<Positive<'a>, Error> {
    ring::io::der::positive_integer(input).map_err(|_| Error::BadDER)
}

pub(super) fn time_choice(input: &mut untrusted::Reader<'_>) -> Result<Time, Error> {
    let is_utc_time = input.peek(Tag::UTCTime as u8);
    let expected_tag = if is_utc_time {
        Tag::UTCTime
    } else {
        Tag::GeneralizedTime
    };

    fn read_digit(inner: &mut untrusted::Reader<'_>) -> Result<u64, Error> {
        let b = inner.read_byte().map_err(|_| Error::BadDERTime)?;
        if b < b'0' || b > b'9' {
            return Err(Error::BadDERTime);
        }
        Ok((b - b'0') as u64)
    }

    fn read_two_digits(
        inner: &mut untrusted::Reader<'_>, min: u64, max: u64,
    ) -> Result<u64, Error> {
        let hi = read_digit(inner)?;
        let lo = read_digit(inner)?;
        let value = (hi * 10) + lo;
        if value < min || value > max {
            return Err(Error::BadDERTime);
        }
        Ok(value)
    }

    nested(input, expected_tag, Error::BadDER, |value| {
        let (year_hi, year_lo) = if is_utc_time {
            let lo = read_two_digits(value, 0, 99)?;
            let hi = if lo >= 50 { 19 } else { 20 };
            (hi, lo)
        } else {
            let hi = read_two_digits(value, 0, 99)?;
            let lo = read_two_digits(value, 0, 99)?;
            (hi, lo)
        };

        let year = (year_hi * 100) + year_lo;
        let month = read_two_digits(value, 1, 12)?;
        let days_in_month = calendar::days_in_month(year, month);
        let day_of_month = read_two_digits(value, 1, days_in_month)?;
        let hours = read_two_digits(value, 0, 23)?;
        let minutes = read_two_digits(value, 0, 59)?;
        let seconds = read_two_digits(value, 0, 59)?;

        let time_zone = value.read_byte().map_err(|_| Error::BadDERTime)?;
        if time_zone != b'Z' {
            return Err(Error::BadDERTime);
        }

        calendar::time_from_ymdhms_utc(year, month, day_of_month, hours, minutes, seconds)
    })
}
