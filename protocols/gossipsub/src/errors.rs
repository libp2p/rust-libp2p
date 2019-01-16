use std::{error, fmt, io};

#[derive(Debug)]
pub enum GError {
    Io(io::Error),
    NotSubscribedToTopic(String, String),
    TopicNotInMesh(String, String),
}

impl fmt::Display for GError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            GError::Io(ref err) => write!(f, "IO error: {}", err),
            GError::NotSubscribedToTopic(ref th, ref peer) => write!(f,
                "TopicNotInSubTopics: topic hash: {}, peer_id: {}", th, peer),
            GError::TopicNotInMesh(ref th, ref peer) => write!(f,
                "TopicNotInMesh: topic hash: {}, peer_id: {}", th, peer),
        }
    }
}

impl error::Error for GError {}
