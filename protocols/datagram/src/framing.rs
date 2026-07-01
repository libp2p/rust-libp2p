//! Datagram wire format ([libp2p/specs#680]): QUIC varint control-stream id,
//! then the payload.
//!
//! [libp2p/specs#680]: https://github.com/libp2p/specs/pull/680

use bytes::{BufMut, Bytes, BytesMut};

/// Frame `payload` for the flow keyed by `control_stream_id`.
pub(crate) fn frame(control_stream_id: u64, payload: &[u8]) -> Bytes {
    let mut buf = BytesMut::with_capacity(8 + payload.len());
    put_varint(control_stream_id, &mut buf);
    buf.put_slice(payload);
    buf.freeze()
}

/// Split a datagram into its control-stream id and payload, or `None` if the
/// leading varint is truncated.
pub(crate) fn parse(datagram: &[u8]) -> Option<(u64, Bytes)> {
    let (id, len) = get_varint(datagram)?;
    Some((id, Bytes::copy_from_slice(&datagram[len..])))
}

/// QUIC varint encoding (RFC 9000 section 16).
fn put_varint(value: u64, out: &mut BytesMut) {
    if value < (1 << 6) {
        out.put_u8(value as u8);
    } else if value < (1 << 14) {
        out.put_u16(value as u16 | (0b01 << 14));
    } else if value < (1 << 30) {
        out.put_u32(value as u32 | (0b10 << 30));
    } else {
        debug_assert!(value < (1 << 62), "stream ids fit in 62 bits");
        out.put_u64(value | (0b11 << 62));
    }
}

/// Returns the decoded value and the number of bytes it consumed.
fn get_varint(buf: &[u8]) -> Option<(u64, usize)> {
    let first = *buf.first()?;
    let len = 1usize << (first >> 6);
    if buf.len() < len {
        return None;
    }
    let mut value = u64::from(first & 0x3f);
    for &b in &buf[1..len] {
        value = (value << 8) | u64::from(b);
    }
    Some((value, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip_across_lengths() {
        for value in [0, 63, 64, 16_383, 16_384, 1 << 29, 1 << 30, (1 << 62) - 1] {
            let mut buf = BytesMut::new();
            put_varint(value, &mut buf);
            assert_eq!(get_varint(&buf), Some((value, buf.len())));
        }
    }

    #[test]
    fn frame_then_parse_preserves_payload() {
        let framed = frame(4611686018427387903, b"payload");
        assert_eq!(
            parse(&framed),
            Some((4611686018427387903, Bytes::from_static(b"payload")))
        );
    }

    #[test]
    fn empty_payload_is_valid() {
        assert_eq!(parse(&frame(7, b"")), Some((7, Bytes::new())));
    }

    #[test]
    fn truncated_id_is_rejected() {
        assert_eq!(parse(&[]), None);
        assert_eq!(parse(&[0b1100_0000]), None);
    }
}
