use crate::error::ValidationError;
use crate::peer_score::RejectReason;
use crate::MessageId;
use libp2p_core::PeerId;
use log::debug;
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::collections::HashMap;
use wasm_timer::Instant;

///struct that tracks recently sent iwant messages and checks if peers respond to them
///for each iwant message we track one random requested message id
#[derive(Default)]
pub(crate) struct GossipPromises {
    // stores for each tracked message id and peer the instant when this promise expires
    // if the peer didn't respond until then we consider the promise as broken and penalize the
    // peer.
    promises: HashMap<MessageId, HashMap<PeerId, Instant>>,
}

impl GossipPromises {
    /// Track a promise to deliver a message from a list of msgIDs we are requesting.
    pub fn add_promise(&mut self, peer: PeerId, messages: &Vec<MessageId>, expires: Instant) {
        //randomly select a message id
        let mut rng = thread_rng();
        if let Some(message_id) = messages.choose(&mut rng) {
            //if a promise for this message id and peer already exists we don't update expires!
            self.promises
                .entry(message_id.clone())
                .or_insert_with(|| HashMap::new())
                .entry(peer.clone())
                .or_insert(expires);
        }
    }

    pub fn deliver_message(&mut self, message_id: &MessageId) {
        //someone delivered a message, we can stop tracking all promises for it
        self.promises.remove(message_id);
    }

    pub fn reject_message(&mut self, message_id: &MessageId, reason: &RejectReason) {
        // A message got rejected, so we can stop tracking promises and let the score penalty apply
        // from invalid message delivery.
        // We do take exception and apply promise penalty regardless in the following cases, where
        // the peer delivered an obviously invalid message.
        match reason {
            RejectReason::ProtocolValidationError(ValidationError::InvalidSignature) => return,
            RejectReason::SelfOrigin => return,
            _ => self.promises.remove(message_id),
        };
    }

    /// Returns the number of broken promises for each peer who didn't follow up on an IWANT
    /// request.
    /// This should be called not too often relative to the expire times, since it iterates over
    /// the whole stored data.
    pub fn get_broken_promises(&mut self) -> HashMap<PeerId, usize> {
        let now = Instant::now();
        let mut result = HashMap::new();
        self.promises.retain(|msg, peers| {
            peers.retain(|peer_id, expires| {
                if *expires < now {
                    let count = result.entry(peer_id.clone()).or_insert(0);
                    *count += 1;
                    debug!(
                        "The peer {} broke the promise to deliver message {} in time!",
                        peer_id, msg
                    );
                    false
                } else {
                    true
                }
            });
            !peers.is_empty()
        });
        result
    }
}
