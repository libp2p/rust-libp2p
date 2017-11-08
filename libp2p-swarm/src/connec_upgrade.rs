use tokio_io::{AsyncRead, AsyncWrite};

pub trait ConnectionUpgrade<C> {
    type Output: AsyncRead + AsyncWrite;
    fn upgrade(&self, i: C) -> Self::Output;
}

// TODO: move `PlainText` to a separate crate?
pub struct PlainText;

impl<C> ConnectionUpgrade<C> for PlainText
    where C: AsyncRead + AsyncWrite
{
    type Output = C;
    #[inline]
    fn upgrade(&self, i: C) -> C {
        i
    }
}
