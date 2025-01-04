// Copyright 2023 Protocol Labs.
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

use std::time::Duration;

use futures::{
    future::{select, Either},
    AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, FutureExt, SinkExt, Stream, StreamExt,
};
use futures_timer::Delay;
use web_time::Instant;

use crate::{Final, Intermediate, Run, RunDuration, RunParams, RunUpdate};

const BUF: [u8; 1024] = [0; 1024];
const REPORT_INTERVAL: Duration = Duration::from_secs(1);

pub(crate) fn send_receive<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    params: RunParams,
    stream: S,
) -> impl Stream<Item = Result<RunUpdate, std::io::Error>> {
    // Use a channel to simulate a generator. `send_receive_inner` can `yield` events through the
    // channel.
    let (sender, receiver) = futures::channel::mpsc::channel(0);
    let receiver = receiver.fuse();
    let inner = send_receive_inner(params, stream, sender).fuse();

    futures::stream::select(
        receiver.map(|progressed| Ok(RunUpdate::Intermediate(progressed))),
        inner
            .map(|finished| finished.map(RunUpdate::Final))
            .into_stream(),
    )
}

async fn send_receive_inner<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    params: RunParams,
    mut stream: S,
    mut progress: futures::channel::mpsc::Sender<Intermediate>,
) -> Result<Final, std::io::Error> {
    let mut delay = Delay::new(REPORT_INTERVAL);

    let RunParams {
        to_send,
        to_receive,
    } = params;

    let mut receive_buf = vec![0; 1024];
    let to_receive_bytes = (to_receive as u64).to_be_bytes();
    stream.write_all(&to_receive_bytes).await?;

    let write_start = Instant::now();
    let mut intermittant_start = Instant::now();
    let mut sent = 0;
    let mut intermittent_sent = 0;

    while sent < to_send {
        let n = std::cmp::min(to_send - sent, BUF.len());
        let buf = &BUF[..n];

        let mut write = stream.write(buf);
        sent += loop {
            match select(&mut delay, &mut write).await {
                Either::Left((_, _)) => {
                    delay.reset(REPORT_INTERVAL);
                    progress
                        .send(Intermediate {
                            duration: intermittant_start.elapsed(),
                            sent: sent - intermittent_sent,
                            received: 0,
                        })
                        .await
                        .expect("receiver not to be dropped");
                    intermittant_start = Instant::now();
                    intermittent_sent = sent;
                }
                Either::Right((n, _)) => break n?,
            }
        }
    }

    loop {
        match select(&mut delay, stream.close()).await {
            Either::Left((_, _)) => {
                delay.reset(REPORT_INTERVAL);
                progress
                    .send(Intermediate {
                        duration: intermittant_start.elapsed(),
                        sent: sent - intermittent_sent,
                        received: 0,
                    })
                    .await
                    .expect("receiver not to be dropped");
                intermittant_start = Instant::now();
                intermittent_sent = sent;
            }
            Either::Right((Ok(_), _)) => break,
            Either::Right((Err(e), _)) => return Err(e),
        }
    }

    let write_done = Instant::now();
    let mut received = 0;
    let mut intermittend_received = 0;

    while received < to_receive {
        let mut read = stream.read(&mut receive_buf);
        received += loop {
            match select(&mut delay, &mut read).await {
                Either::Left((_, _)) => {
                    delay.reset(REPORT_INTERVAL);
                    progress
                        .send(Intermediate {
                            duration: intermittant_start.elapsed(),
                            sent: sent - intermittent_sent,
                            received: received - intermittend_received,
                        })
                        .await
                        .expect("receiver not to be dropped");
                    intermittant_start = Instant::now();
                    intermittent_sent = sent;
                    intermittend_received = received;
                }
                Either::Right((n, _)) => break n?,
            }
        }
    }

    let read_done = Instant::now();

    Ok(Final {
        duration: RunDuration {
            upload: write_done.duration_since(write_start),
            download: read_done.duration_since(write_done),
        },
    })
}

pub(crate) async fn receive_send<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
) -> Result<Run, std::io::Error> {
    let to_send = {
        let mut buf = [0; 8];
        stream.read_exact(&mut buf).await?;

        u64::from_be_bytes(buf) as usize
    };

    let read_start = Instant::now();

    let mut receive_buf = vec![0; 1024];
    let mut received = 0;
    loop {
        let n = stream.read(&mut receive_buf).await?;
        received += n;
        if n == 0 {
            break;
        }
    }

    let read_done = Instant::now();

    let mut sent = 0;
    while sent < to_send {
        let n = std::cmp::min(to_send - sent, BUF.len());
        let buf = &BUF[..n];

        sent += stream.write(buf).await?;
    }

    stream.close().await?;
    let write_done = Instant::now();

    Ok(Run {
        params: RunParams {
            to_send: sent,
            to_receive: received,
        },
        duration: RunDuration {
            upload: write_done.duration_since(read_done),
            download: read_done.duration_since(read_start),
        },
    })
}
