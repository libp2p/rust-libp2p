use bytes::{BufMut, BytesMut};

/// An [`io::Write`] impl for [`BytesMut`].
///
/// In contrast to [`bytes::buf::Writer`] this [`io::Write] implementation
/// transparently reserves enough space for [`io::Write::write_all`] to
/// succeed, i.e. it does not require upfront reservation of space.
pub(crate) struct BytesWriter(pub(crate) BytesMut);

impl std::io::Write for BytesWriter {
    fn write(&mut self, src: &[u8]) -> std::io::Result<usize> {
        let n = std::cmp::min(self.0.remaining_mut(), src.len());
        self.0.put(&src[.. n]);
        Ok(n)
    }

    fn write_all(&mut self, mut buf: &[u8]) -> std::io::Result<()> {
        if self.0.remaining_mut() < buf.len() {
            self.0.reserve(buf.len() - self.0.remaining_mut());
        }
        while !buf.is_empty() {
            match self.write(buf) {
                Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
                Ok(n) => buf = &buf[n ..],
                Err(e) => if e.kind() != std::io::ErrorKind::Interrupted {
                    return Err(e)
                }
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}


