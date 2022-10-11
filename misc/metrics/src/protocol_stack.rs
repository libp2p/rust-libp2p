use libp2p_core::multiaddr::{Multiaddr,Protocol};

//TODO: remove this file and replace with calls to Multiaddr::protocol_stack
//  once that lands : https://github.com/multiformats/rust-multiaddr/pull/60
//  This does imply changing the keys from Vec<String> to Vec<'static str>

fn tag(proto: Protocol) -> Option<String> {
    format!("{}", proto).split("/").skip(1).next().map(str::to_owned)
}

pub(crate) fn protocol_stack(ma: &Multiaddr) -> Vec<String> {
    ma.iter().filter_map(tag).collect()
}
