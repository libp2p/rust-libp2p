const ASN1_CONSTRUCTED_FLAG: u8 = 0x20;

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Tag {
    Bool = 0x1,
    Integer = 0x2,
    BitString = 0x3,
    OctetString = 0x4,
    ObjectIdentifier = 0x6,
    PrintableString = 0x13,
    UTCTime = 0x17,
    Sequence = (0x10 | ASN1_CONSTRUCTED_FLAG),
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct InvalidValue;

impl Tag {
    pub fn try_from(val: u8) -> Result<Self, InvalidValue> {
        const SEQ: u8 = 0x10 | ASN1_CONSTRUCTED_FLAG;

        match val {
            0x1 => Ok(Tag::Bool),
            0x2 => Ok(Tag::Integer),
            0x3 => Ok(Tag::BitString),
            0x4 => Ok(Tag::OctetString),
            0x6 => Ok(Tag::ObjectIdentifier),
            0x13 => Ok(Tag::PrintableString),
            0x17 => Ok(Tag::UTCTime),
            SEQ => Ok(Tag::Sequence),
            _ => Err(InvalidValue),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Tag;

    #[test]
    fn try_from_works() {
        for &tag in &[
            Tag::Bool,
            Tag::Integer,
            Tag::BitString,
            Tag::OctetString,
            Tag::ObjectIdentifier,
            Tag::PrintableString,
            Tag::UTCTime,
            Tag::Sequence,
        ] {
            assert_eq!(Tag::try_from(tag as u8), Ok(tag))
        }
    }
}
