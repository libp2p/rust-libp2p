use bytes::BytesMut;

/// An [`io::Write`] impl for [`BytesMut`].
///
/// In contrast to [`bytes::buf::Writer`] this [`io::Write] implementation
/// transparently reserves enough space for [`io::Write::write_all`] to
/// succeed, i.e. it does not require upfront reservation of space.
pub(crate) struct BytesWriter(pub(crate) BytesMut);

impl std::io::Write for BytesWriter {
    fn write(&mut self, src: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(src);
        Ok(src.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}


