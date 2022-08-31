use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::SinkExt;
use futures_lite::StreamExt;
use std::io;
use std::task::{Context, Poll};

pub fn new<Req, Res>(capacity: usize) -> (Sender<Req, Res>, Receiver<Req, Res>) {
    let (sender, receiver) = mpsc::channel(capacity);

    (
        Sender {
            inner: futures::lock::Mutex::new(sender),
        },
        Receiver { inner: receiver },
    )
}

pub struct Sender<Req, Res> {
    inner: futures::lock::Mutex<mpsc::Sender<(Req, oneshot::Sender<Res>)>>,
}

impl<Req, Res> Sender<Req, Res> {
    pub async fn send(&self, req: Req) -> io::Result<Res> {
        let (sender, receiver) = oneshot::channel();

        self.inner
            .lock()
            .await
            .send((req, sender))
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let res = receiver
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Ok(res)
    }
}

pub struct Receiver<Req, Res> {
    inner: mpsc::Receiver<(Req, oneshot::Sender<Res>)>,
}

impl<Req, Res> Receiver<Req, Res> {
    pub fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<(Req, oneshot::Sender<Res>)>> {
        self.inner.poll_next(cx)
    }
}
