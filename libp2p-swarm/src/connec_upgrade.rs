use tokio_io::{AsyncRead, AsyncWrite};
use AsyncReadWrite;

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

pub trait AbstractConnUpgr { fn upgrade(&self, Box<AsyncReadWrite>) -> Box<AsyncReadWrite>; }
impl<T> AbstractConnUpgr for T where T: ConnectionUpgrade<Box<AsyncReadWrite>>, T::Output: 'static {
    #[inline]
    fn upgrade(&self, i: Box<AsyncReadWrite>) -> Box<AsyncReadWrite> {
        Box::new(self.upgrade(i))
    }
}
