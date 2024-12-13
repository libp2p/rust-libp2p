// Copyright 2022 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use std::{
    io,
    task::{Context, Poll},
};

use futures::{
    channel::{mpsc, oneshot},
    SinkExt, StreamExt,
};

pub(crate) fn new<Req, Res>(capacity: usize) -> (Sender<Req, Res>, Receiver<Req, Res>) {
    let (sender, receiver) = mpsc::channel(capacity);

    (
        Sender {
            inner: futures::lock::Mutex::new(sender),
        },
        Receiver { inner: receiver },
    )
}

pub(crate) struct Sender<Req, Res> {
    inner: futures::lock::Mutex<mpsc::Sender<(Req, oneshot::Sender<Res>)>>,
}

impl<Req, Res> Sender<Req, Res> {
    pub(crate) async fn send(&self, req: Req) -> io::Result<Res> {
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

pub(crate) struct Receiver<Req, Res> {
    inner: mpsc::Receiver<(Req, oneshot::Sender<Res>)>,
}

impl<Req, Res> Receiver<Req, Res> {
    pub(crate) fn poll_next_unpin(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<(Req, oneshot::Sender<Res>)>> {
        self.inner.poll_next_unpin(cx)
    }
}
