use crate::{provider::Provider, Error};

use futures::future::Either;

use rand::{distributions, Rng};

use std::convert::Infallible;
use std::{
    net::{SocketAddr, UdpSocket},
    time::Duration,
};

pub(crate) async fn hole_puncher<P: Provider>(
    socket: UdpSocket,
    remote_addr: SocketAddr,
    timeout_duration: Duration,
) -> Error {
    let punch_holes_future = punch_holes::<P>(socket, remote_addr);
    futures::pin_mut!(punch_holes_future);
    match futures::future::select(P::sleep(timeout_duration), punch_holes_future).await {
        Either::Left(_) => Error::HandshakeTimedOut,
        Either::Right((Err(hole_punch_err), _)) => hole_punch_err,
        Either::Right((Ok(never), _)) => match never {},
    }
}

async fn punch_holes<P: Provider>(
    socket: UdpSocket,
    remote_addr: SocketAddr,
) -> Result<Infallible, Error> {
    loop {
        let contents: Vec<u8> = rand::thread_rng()
            .sample_iter(distributions::Standard)
            .take(64)
            .collect();

        tracing::trace!("Sending random UDP packet to {remote_addr}");

        P::send_to(&socket, &contents, remote_addr).await?;

        let sleep_duration = Duration::from_millis(rand::thread_rng().gen_range(10..=200));
        P::sleep(sleep_duration).await;
    }
}
