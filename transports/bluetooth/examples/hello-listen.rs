use futures::prelude::*;
use libp2p_core::Transport;

fn main() {
    let config = libp2p_bluetooth::BluetoothConfig::default();

    // TODO: correct address
    let listener = config.clone().listen_on("/bluetooth/00:00:00:00:00:00/l2cap/3/rfcomm/5".parse().unwrap()).unwrap().0;

    let listener = listener.into_future().map_err(|(err, _)| err).map(|(inc, _)| inc.unwrap().0).map(|_| ());

    tokio::runtime::Runtime::new().unwrap().block_on(listener).unwrap();

    println!("it's working! working!");
}
