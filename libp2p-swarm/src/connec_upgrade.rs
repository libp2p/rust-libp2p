use std::io::Error as IoError;
use tokio_io::{AsyncRead, AsyncWrite};
use futures::future::{Future, ok as future_ok, FutureResult};
use AsyncReadWrite;

pub trait ConnectionUpgrade<C> {
    type Output: AsyncRead + AsyncWrite;
    type Future: Future<Item = Self::Output, Error = IoError>;
    fn upgrade(&self, i: C) -> Self::Future;
}

// TODO: move `PlainText` to a separate crate?
pub struct PlainText;

impl<C> ConnectionUpgrade<C> for PlainText
    where C: AsyncRead + AsyncWrite
{
    type Output = C;
    type Future = FutureResult<C, IoError>;

    #[inline]
    fn upgrade(&self, i: C) -> Self::Future {
        future_ok(i)
    }
}

pub trait AbstractConnUpgr { fn upgrade(&self, Box<AsyncReadWrite>) -> Box<Future<Item = Box<AsyncReadWrite>, Error = IoError>>; }
impl<T> AbstractConnUpgr for T where T: ConnectionUpgrade<Box<AsyncReadWrite>>, T::Output: 'static, T::Future: 'static {
    #[inline]
    fn upgrade(&self, i: Box<AsyncReadWrite>) -> Box<Future<Item = Box<AsyncReadWrite>, Error = IoError>> {
        Box::new(self.upgrade(i).map(|i| Box::new(i) as Box<AsyncReadWrite>))
    }
}
