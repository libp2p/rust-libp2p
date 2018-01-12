use byteorder::ReadBytesExt;
use common::Tag;
use smallvec::Array;
use std::convert;
use std::io::{self, BufRead, Cursor};
use utils::{BitString, Integer, ObjectIdentifier};

#[derive(Debug, PartialEq, Eq)]
pub enum DeserializationError {
    UnexpectedTag {
        expected: Tag,
        actual: (u8, Option<Tag>),
    },
    ShortData,
    ExtraData,
    IntegerOverflow,
    InvalidValue,
}

impl convert::From<io::Error> for DeserializationError {
    fn from(e: io::Error) -> DeserializationError {
        return match e.kind() {
            io::ErrorKind::UnexpectedEof => DeserializationError::ShortData,
            _ => panic!("Unexpected error!"),
        };
    }
}

pub type DeserializationResult<T> = Result<T, DeserializationError>;

fn read_base128_int(reader: &mut Cursor<&[u8]>) -> DeserializationResult<u32> {
    let mut ret = 0u32;
    for _ in 0..4 {
        let b = reader.read_u8()?;
        ret <<= 7;
        ret |= (b & 0x7f) as u32;
        if b & 0x80 == 0 {
            return Ok(ret);
        }
    }
    return Err(DeserializationError::InvalidValue);
}

pub struct Deserializer<'a> {
    reader: Cursor<&'a [u8]>,
}

fn read_length(reader: &mut Cursor<&[u8]>) -> DeserializationResult<usize> {
    let b = reader.read_u8()?;

    if b & 0x80 == 0 {
        return Ok((b & 0x7f) as usize);
    }

    let num_bytes = b & 0x7f;
    // Indefinite lengths are not valid DER.
    if num_bytes == 0 {
        return Err(DeserializationError::InvalidValue);
    }
    let mut length = 0;
    for _ in 0..num_bytes {
        let b = reader.read_u8()?;
        // Handle overflows
        if length > (usize::max_value() >> 8) {
            return Err(DeserializationError::IntegerOverflow);
        }
        length <<= 8;
        length |= b as usize;
        // Disallow leading 0s.
        if length == 0 {
            return Err(DeserializationError::InvalidValue);
        }
    }

    // Do not allow values <127 to be encoded using the long form
    if length < 128 {
        return Err(DeserializationError::InvalidValue);
    }

    Ok(length)
}

impl<'a> Deserializer<'a> {
    pub fn new(data: &[u8]) -> Deserializer {
        return Deserializer {
            reader: Cursor::new(data),
        };
    }

    fn read_with_tag<O, T, F>(&mut self, expected_tag: O, body: F) -> DeserializationResult<T>
    where
        F: Fn(&[u8]) -> DeserializationResult<T>,
        O: Into<Option<Tag>>,
    {
        let mut reader = self.reader.clone();
        let expected_tag = expected_tag.into();
        let tag = reader.read_u8()?;

        // TODO: only some of the bits in the first byte are for the tag
        if let Some(expected_tag) = expected_tag {
            let expected_byte = expected_tag as u8;
            if tag != expected_byte {
                return Err(DeserializationError::UnexpectedTag {
                    expected: expected_tag,
                    actual: (tag, Tag::try_from(tag).ok()),
                });
            }
        }

        let length = read_length(&mut reader)?;

        let out = {
            let buf = reader.fill_buf().unwrap();
            if buf.len() < length {
                return Err(DeserializationError::ShortData);
            }
            body(&buf[..length])?
        };

        reader.consume(length);

		self.reader = reader;

        Ok(out)
    }

    pub fn finish(self) -> DeserializationResult<()> {
        if self.reader.position() as usize != self.reader.get_ref().len() {
            return Err(DeserializationError::ExtraData);
        }
        return Ok(());
    }

    pub fn read_bool(&mut self) -> DeserializationResult<bool> {
        return self.read_with_tag(Tag::Bool, |data| {
            if data == b"\x00" {
                return Ok(false);
            } else if data == b"\xff" {
                return Ok(true);
            } else {
                return Err(DeserializationError::InvalidValue);
            }
        });
    }

    pub fn ignore(&mut self) -> DeserializationResult<()> {
        return self.read_with_tag(None, |_| Ok(()));
    }

    pub fn read_int<T>(&mut self) -> DeserializationResult<T>
    where
        T: Integer,
    {
        println!("{:?}", self.reader.position());
        return self.read_with_tag(Tag::Integer, |data| {
            if data.len() > 1 {
                match (data[0], data[1] & 0x80) {
                    (0xff, 0x80) | (0x00, 0x00) => return Err(DeserializationError::InvalidValue),
                    _ => {}
                }
            }
            return T::decode(data);
        });
    }

    pub fn read_int_bytes<T: Array<Item = u8>>(&mut self) -> DeserializationResult<T> {
        use std::{mem, ptr};

        println!("{:?}", self.reader.position());
        self.read_with_tag(Tag::Integer, |data| {
            // TODO: Why do all integers have an extra 0 byte at the start?
            if data.len() != T::size() + 1 {
                println!("{:?}", (data.len(), T::size() + 1));
                Err(DeserializationError::InvalidValue)
            } else {
                unsafe {
                    let mut uninit: T = mem::uninitialized();

                    ptr::copy_nonoverlapping(data.as_ptr().offset(1), uninit.ptr_mut(), T::size());

                    Ok(uninit)
                }
            }
        })
    }

    pub fn read_octet_string(&mut self) -> DeserializationResult<Vec<u8>> {
        return self.read_with_tag(Tag::OctetString, |data| {
            return Ok(data.to_owned());
        });
    }

    pub fn read_bit_string(&mut self) -> DeserializationResult<BitString> {
        return self.read_with_tag(Tag::BitString, |data| {
            let padding_bits = match data.get(0) {
                Some(&bits) => bits,
                None => return Err(DeserializationError::InvalidValue),
            };

            if padding_bits > 7 || (data.len() == 1 && padding_bits > 0) {
                return Err(DeserializationError::InvalidValue);
            }

            return BitString::new(
                data[1..].to_vec(),
                (data.len() - 1) * 8 - (padding_bits as usize),
            ).ok_or(DeserializationError::InvalidValue);
        });
    }

    pub fn read_object_identifier(&mut self) -> DeserializationResult<ObjectIdentifier> {
        return self.read_with_tag(Tag::ObjectIdentifier, |data| {
            if data.is_empty() {
                return Err(DeserializationError::InvalidValue);
            }
            let mut reader = Cursor::new(data);
            let mut s = vec![];
            let v = read_base128_int(&mut reader)?;

            if v < 80 {
                s.push(v / 40);
                s.push(v % 40);
            } else {
                s.push(2);
                s.push(v - 80);
            }

            while (reader.position() as usize) < reader.get_ref().len() {
                s.push(read_base128_int(&mut reader)?);
            }

            return Ok(ObjectIdentifier::new(s).unwrap());
        });
    }

    pub fn read_sequence<F, T>(&mut self, v: F) -> DeserializationResult<T>
    where
        F: Fn(&mut Deserializer) -> DeserializationResult<T>,
    {
        self.read_with_tag(Tag::Sequence, |data| deserialize(data, &v))
    }
}

pub fn deserialize<F, T>(data: &[u8], f: F) -> DeserializationResult<T>
where
    F: Fn(&mut Deserializer) -> DeserializationResult<T>,
{
    let mut deserializer = Deserializer::new(data);
    let result = f(&mut deserializer)?;
    deserializer.finish()?;
    return Ok(result);
}

#[cfg(test)]
mod tests {
    use super::{deserialize, DeserializationError, DeserializationResult, Deserializer};
    use num::{BigInt, FromPrimitive, One};
    use std;
    use std::fmt;
    use utils::{BitString, ObjectIdentifier};

    fn assert_deserializes<T, F>(values: Vec<(DeserializationResult<T>, &[u8])>, f: F)
    where
        T: Eq + fmt::Debug,
        F: Fn(&mut Deserializer) -> DeserializationResult<T>,
    {
        for (expected, value) in values {
            let result = deserialize(value, &f);
            assert_eq!(result, expected);
        }
    }
    #[test]
    fn test_read_extra_data() {
        assert_deserializes(
            vec![(Err(DeserializationError::ExtraData), b"\x00")],
            |_| {
                return Ok(());
            },
        );
    }

    #[test]
    fn test_read_bool() {
        assert_deserializes(
            vec![
                (Ok(true), b"\x01\x01\xff"),
                (Ok(false), b"\x01\x01\x00"),
                (Err(DeserializationError::InvalidValue), b"\x01\x00"),
                (Err(DeserializationError::InvalidValue), b"\x01\x01\x01"),
                (Err(DeserializationError::InvalidValue), b"\x01\x02\x00\x00"),
                (Err(DeserializationError::InvalidValue), b"\x01\x02\xff\x01"),
            ],
            |deserializer| {
                return deserializer.read_bool();
            },
        );
    }

    #[test]
    fn test_read_int_i64() {
        assert_deserializes(
            vec![
                (Ok(0), b"\x02\x01\x00"),
                (Ok(127), b"\x02\x01\x7f"),
                (Ok(128), b"\x02\x02\x00\x80"),
                (Ok(256), b"\x02\x02\x01\x00"),
                (Ok(-128), b"\x02\x01\x80"),
                (Ok(-129), b"\x02\x02\xff\x7f"),
                (Ok(-256), b"\x02\x02\xff\x00"),
                (
                    Ok(std::i64::MAX),
                    b"\x02\x08\x7f\xff\xff\xff\xff\xff\xff\xff",
                ),
                (
                    Err(DeserializationError::UnexpectedTag {
                        expected: ::common::Tag::Integer,
                        actual: (0x3, Some(::common::Tag::BitString)),
                    }),
                    b"\x03",
                ),
                (Err(DeserializationError::ShortData), b"\x02\x02\x00"),
                (Err(DeserializationError::ShortData), b""),
                (Err(DeserializationError::ShortData), b"\x02"),
                (
                    Err(DeserializationError::IntegerOverflow),
                    b"\x02\x09\x02\x00\x00\x00\x00\x00\x00\x00\x00",
                ),
                (
                    Err(DeserializationError::InvalidValue),
                    b"\x02\x05\x00\x00\x00\x00\x01",
                ),
                (Err(DeserializationError::InvalidValue), b"\x02\x02\xff\x80"),
                (Err(DeserializationError::InvalidValue), b"\x02\x00"),
            ],
            |deserializer| {
                return deserializer.read_int();
            },
        );
    }

    #[test]
    fn test_read_int_i32() {
        assert_deserializes(
            vec![
                (Ok(0i32), b"\x02\x01\x00"),
                (Ok(127i32), b"\x02\x01\x7f"),
                (Ok(128i32), b"\x02\x02\x00\x80"),
                (Ok(256i32), b"\x02\x02\x01\x00"),
                (Ok(-128i32), b"\x02\x01\x80"),
                (Ok(-129i32), b"\x02\x02\xff\x7f"),
                (Ok(-256i32), b"\x02\x02\xff\x00"),
                (Ok(std::i32::MAX), b"\x02\x04\x7f\xff\xff\xff"),
                (
                    Err(DeserializationError::IntegerOverflow),
                    b"\x02\x05\x02\x00\x00\x00\x00",
                ),
                (Err(DeserializationError::InvalidValue), b"\x02\x00"),
            ],
            |deserializer| {
                return deserializer.read_int();
            },
        );
    }

    #[test]
    fn test_read_int_i8() {
        assert_deserializes(
            vec![
                (Ok(0i8), b"\x02\x01\x00"),
                (Ok(127i8), b"\x02\x01\x7f"),
                (Ok(-128i8), b"\x02\x01\x80"),
                (
                    Err(DeserializationError::IntegerOverflow),
                    b"\x02\x02\x02\x00",
                ),
                (Err(DeserializationError::InvalidValue), b"\x02\x00"),
            ],
            |deserializer| {
                return deserializer.read_int();
            },
        );
    }

    #[test]
    fn test_read_int_bigint() {
        assert_deserializes(
            vec![
                (Ok(BigInt::from_i64(0).unwrap()), b"\x02\x01\x00"),
                (Ok(BigInt::from_i64(127).unwrap()), b"\x02\x01\x7f"),
                (Ok(BigInt::from_i64(128).unwrap()), b"\x02\x02\x00\x80"),
                (Ok(BigInt::from_i64(256).unwrap()), b"\x02\x02\x01\x00"),
                (Ok(BigInt::from_i64(-128).unwrap()), b"\x02\x01\x80"),
                (Ok(BigInt::from_i64(-129).unwrap()), b"\x02\x02\xff\x7f"),
                (Ok(BigInt::from_i64(-256).unwrap()), b"\x02\x02\xff\x00"),
                (
                    Ok(BigInt::from_i64(std::i64::MAX).unwrap()),
                    b"\x02\x08\x7f\xff\xff\xff\xff\xff\xff\xff",
                ),
                (
                    Ok(BigInt::from_i64(std::i64::MAX).unwrap() + BigInt::one()),
                    b"\x02\x09\x00\x80\x00\x00\x00\x00\x00\x00\x00",
                ),
                (Err(DeserializationError::InvalidValue), b"\x02\x00"),
            ],
            |deserializer| {
                return deserializer.read_int();
            },
        );
    }

    #[test]
    fn test_read_octet_string() {
        assert_deserializes(vec![
            (Ok(b"".to_vec()), b"\x04\x00"),
            (Ok(b"\x01\x02\x03".to_vec()), b"\x04\x03\x01\x02\x03"),
            (
                Ok(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_vec()),
                b"\x04\x81\x81aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            ),
            (Err(DeserializationError::InvalidValue), b"\x04\x80"),
            (Err(DeserializationError::InvalidValue), b"\x04\x81\x00"),
            (Err(DeserializationError::InvalidValue), b"\x04\x81\x01\x09"),
            (
                Err(DeserializationError::IntegerOverflow),
                b"\x04\x89\x01\x01\x01\x01\x01\x01\x01\x01\x01"
            ),
            (Err(DeserializationError::ShortData), b"\x04\x03\x01\x02"),
            (Err(DeserializationError::ShortData), b"\x04\x86\xff\xff\xff\xff\xff\xff"),
        ], |deserializer| {
            return deserializer.read_octet_string();
        });
    }

    #[test]
    fn test_read_bit_string() {
        assert_deserializes(
            vec![
                (
                    Ok(BitString::new(b"".to_vec(), 0).unwrap()),
                    b"\x03\x01\x00",
                ),
                (
                    Ok(BitString::new(b"\x00".to_vec(), 1).unwrap()),
                    b"\x03\x02\x07\x00",
                ),
                (
                    Ok(BitString::new(b"\x80".to_vec(), 1).unwrap()),
                    b"\x03\x02\x07\x80",
                ),
                (
                    Ok(BitString::new(b"\x81\xf0".to_vec(), 12).unwrap()),
                    b"\x03\x03\x04\x81\xf0",
                ),
                (Err(DeserializationError::InvalidValue), b"\x03\x00"),
                (Err(DeserializationError::InvalidValue), b"\x03\x02\x07\x01"),
                (Err(DeserializationError::InvalidValue), b"\x03\x02\x07\x40"),
                (Err(DeserializationError::InvalidValue), b"\x03\x02\x08\x00"),
            ],
            |deserializer| {
                return deserializer.read_bit_string();
            },
        )
    }

    #[test]
    fn test_read_object_identifier() {
        assert_deserializes(
            vec![
                (
                    Ok(ObjectIdentifier::new(vec![2, 5]).unwrap()),
                    b"\x06\x01\x55",
                ),
                (
                    Ok(ObjectIdentifier::new(vec![2, 5, 2]).unwrap()),
                    b"\x06\x02\x55\x02",
                ),
                (
                    Ok(ObjectIdentifier::new(vec![1, 2, 840, 113549]).unwrap()),
                    b"\x06\x06\x2a\x86\x48\x86\xf7\x0d",
                ),
                (
                    Ok(ObjectIdentifier::new(vec![1, 2, 3, 4]).unwrap()),
                    b"\x06\x03\x2a\x03\x04",
                ),
                (
                    Ok(ObjectIdentifier::new(vec![1, 2, 840, 133549, 1, 1, 5]).unwrap()),
                    b"\x06\x09\x2a\x86\x48\x88\x93\x2d\x01\x01\x05",
                ),
                (
                    Ok(ObjectIdentifier::new(vec![2, 100, 3]).unwrap()),
                    b"\x06\x03\x81\x34\x03",
                ),
                (Err(DeserializationError::InvalidValue), b"\x06\x00"),
                (
                    Err(DeserializationError::InvalidValue),
                    b"\x06\x07\x55\x02\xc0\x80\x80\x80\x80",
                ),
                (Err(DeserializationError::ShortData), b"\x06\x02\x2a\x86"),
            ],
            |deserializer| {
                return deserializer.read_object_identifier();
            },
        );
    }

    #[test]
    fn test_read_sequence() {
        assert_deserializes(
            vec![
                (Ok((1, 2)), b"\x30\x06\x02\x01\x01\x02\x01\x02"),
                (
                    Err(DeserializationError::ShortData),
                    b"\x30\x03\x02\x01\x01",
                ),
                (
                    Err(DeserializationError::ExtraData),
                    b"\x30\x07\x02\x01\x01\x02\x01\x02\x00",
                ),
            ],
            |deserializer| {
                return deserializer.read_sequence(|deserializer| {
                    return Ok((deserializer.read_int()?, deserializer.read_int()?));
                });
            },
        );
    }
}
