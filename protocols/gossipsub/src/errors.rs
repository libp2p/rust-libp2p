
use custom_error::custom_error;
use std::io;

custom_error!{pub GError
    Io{source: io::Error} = "Input/output error",
    // Note that when combined with the err arguments passed elsewhere e.g. in mesh and layer, these are repetitive, but avoids ambiguity.
    NotSubscribedToTopic{t_hash: String, peer_id: String, err: String}
        = "The topic with topic hash '{t_hash}' is not in the subscribed \
        topics of the peer with peer id '{peer_id}'.'{err}'",
    NotGraftedToTopic{t_hash: String, peer_id: String, err: String}
        = "The peer with peer id '{peer_id}' is not grafted to the topic. \
        '{err}'",
    TopicNotInMesh{t_hash: String, err: String}
        = "The topic with topic hash '{t_hash}' was not found. '{err}'",
    AlreadyGrafted{t_hash: String, peer_id: String, err: String}
        = "Tried to graft the peer with peer_id '{peer_id}' to the topic \
        with topic hash '{t_hash}' in the mesh, but it is already grafted.",
    InvalidPeerId{from_data: String}
        = "The from field '{from_data}' of an instance of rpc_proto::Message \
        could not be converted to a valid peer ID.",
    NotConnectedToPeer{peer: String, err: String}
        = "The remote peer {peer} was not found in the \
        `connected_peers.gossipsub` of the local peer.",
    // NotEnoughPeers{err: String}
    //     = "The local peer is not connected to enough peers.",
}

pub type Result<T> = std::result::Result<T, GError>;

// use std::{error, fmt, io};

// #[derive(Debug)]
// pub enum GError {
//     Io(io::Error),
//     NotSubscribedToTopic(String, String),

//     // TopicNotInMesh(String, String),
// }

// impl fmt::Display for GError {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//         match *self {
//             GError::Io(ref err) => write!(f, "IO error: {}", err),
//             GError::NotSubscribedToTopic(ref th, ref peer) => write!(f,
//                 "TopicNotInSubTopics: topic hash: {}, peer_id: {}", th, peer),
//             // GError::TopicNotInMesh(ref th, ref peer) => write!(f,
//             //     "TopicNotInMesh: topic hash: {}, peer_id: {}", th, peer),
//         }
//     }
// }

// impl error::Error for GError {}
