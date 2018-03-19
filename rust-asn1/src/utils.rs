use deserializer::{DeserializationError, DeserializationResult};
use num::{BigInt, One, Signed};
use num::bigint::Sign;
use std::mem;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectIdentifier {
    pub parts: Vec<u32>,
}

impl ObjectIdentifier {
    pub fn new(oid: Vec<u32>) -> Option<ObjectIdentifier> {
        if oid.len() < 2 || oid[0] > 2 || (oid[0] < 2 && oid[1] >= 40) {
            return None;
        }

        return Some(ObjectIdentifier { parts: oid });
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitString {
    data: Vec<u8>,
    bit_length: usize,
}

impl BitString {
    pub fn from_bytes(data: Vec<u8>) -> Option<BitString> {
        let length = data.len() * 8;
        return BitString::new(data, length);
    }

    pub fn new(data: Vec<u8>, bit_length: usize) -> Option<BitString> {
        match (data.len(), bit_length) {
            (0, 0) => (),
            (_, 0) | (0, _) => return None,
            (i, j) if (i * 8 < j) || (i - 1) * 8 > j => return None,
            _ => (),
        }

        let padding_bits = data.len() * 8 - bit_length;
        if padding_bits > 0 && data[data.len() - 1] & ((1 << padding_bits) - 1) != 0 {
            return None;
        }

        return Some(BitString {
            data: data,
            bit_length: bit_length,
        });
    }

    pub fn len(&self) -> usize {
        return self.bit_length;
    }

    pub fn as_bytes(&self) -> &[u8] {
        return &self.data;
    }
}

fn _int_length(v: i64) -> usize {
    let mut num_bytes = 1;
    let mut i = v;

    while i > 127 || i < -128 {
        num_bytes += 1;
        i >>= 8;
    }
    return num_bytes;
}

pub trait Integer: Sized {
    fn encode(&self) -> Vec<u8>;
    fn decode(&[u8]) -> DeserializationResult<Self>;
}

macro_rules! primitive_integer {
    ($Int:ident) => {
        impl Integer for $Int {
            fn encode(&self) -> Vec<u8> {
                let n = _int_length(*self as i64);
                let mut result = Vec::with_capacity(n);
                for i in (1..n+1).rev() {
                    result.push((self >> ((i - 1) * 8)) as u8);
                }
                return result;
            }

            fn decode(data: &[u8]) -> DeserializationResult<$Int> {
                if data.len() > mem::size_of::<$Int>() {
                    return Err(DeserializationError::IntegerOverflow);
                } else if data.is_empty() {
                    return Err(DeserializationError::InvalidValue);
                }

                let mut ret = 0;
                for b in data.iter() {
                    ret <<= 8;
                    ret |= *b as i64;
                }
                // Shift up and down in order to sign extend the result.
                ret <<= 64 - data.len() * 8;
                ret >>= 64 - data.len() * 8;
                return Ok(ret as $Int);
            }
        }
    };
    ($($i:ident),*) => {
        $(primitive_integer!($i);)*
    }
}

primitive_integer! {
    i8, i16, i32, i64,
    u8, u16, u32, u64
}

impl Integer for BigInt {
    fn encode(&self) -> Vec<u8> {
        if self.is_positive() {
            let (_, mut bytes) = self.to_bytes_be();
            if bytes[0] & 0x80 == 0x80 {
                // If the data has a leading 0x80, pad with a zero-byte.
                bytes.insert(0, 0);
            }
            return bytes;
        } else if self.is_negative() {
            // Convert negative numbers to two's-complement by subtracting one and inverting.
            let n_minus_1 = -self - BigInt::one();
            let (_, mut bytes) = n_minus_1.to_bytes_be();

            for i in 0..bytes.len() {
                bytes[i] ^= 0xff;
            }

            if bytes[0] & 0x80 == 0 {
                bytes.insert(0, 0xff);
            }
            return bytes;
        } else {
            return b"\x00".to_vec();
        }
    }

    fn decode(data: &[u8]) -> DeserializationResult<BigInt> {
        if data.is_empty() {
            return Err(DeserializationError::InvalidValue);
        }

        if data[0] & 0x80 == 0x80 {
            let inverse_bytes = data.iter().map(|b| !b).collect::<Vec<u8>>();
            let n_minus_1 = BigInt::from_bytes_be(Sign::Plus, &inverse_bytes[..]);
            return Ok(-(n_minus_1 + BigInt::one()));
        } else {
            return Ok(BigInt::from_bytes_be(Sign::Plus, &data[..]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BitString, ObjectIdentifier};

    #[test]
    fn test_object_identifier_new() {
        assert!(ObjectIdentifier::new(vec![]).is_none());
        assert!(ObjectIdentifier::new(vec![3, 10]).is_none());
        assert!(ObjectIdentifier::new(vec![1, 50]).is_none());
    }

    #[test]
    fn test_bit_string_new() {
        assert!(BitString::new(b"".to_vec(), 1).is_none());
        assert!(BitString::new(b"\x00".to_vec(), 0).is_none());
        assert!(BitString::new(b"\x00".to_vec(), 9).is_none());
        assert!(BitString::new(b"\xff".to_vec(), 3).is_none());
    }

    #[test]
    fn test_bit_string_from_bytes() {
        assert_eq!(
            BitString::from_bytes(b"".to_vec()),
            BitString::new(b"".to_vec(), 0)
        );
        assert_eq!(
            BitString::from_bytes(b"abc".to_vec()),
            BitString::new(b"abc".to_vec(), 24)
        );
    }
}
