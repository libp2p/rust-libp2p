//! Manages and stores the Scoring logic of a particular peer on the gossipsub behaviour.

use std::time::Instant;
use crate::TopicHash;

mod params;
use params::*;

struct PeerScore {
    params: PeerScoreParams,
    /// The score parameters.
    peer_stats: HashMap<PeerId, PeerStats>,
    /// Tracking peers per IP.
    peer_ips: HashMap<IpAddr,PeerId>,
    /// Message delivery tracking.
    deliveries: HashMap<String,DeliveryRecord>,
    /// The message id function.
    msg_id: String,
}

/// General statistics for a given gossipsub peer.
struct PeerStats { 
    /// Connection status of the peer.
    connected: bool,
    /// Expiration time of the score state for disconnected peers.
    expired: Instant,
    /// Stats per topic.
    topics: HashMap<TopicHash, TopicStats>,
    /// IP tracking for individual peers.
    known_ips: Vec<IpAddr>,
    /// Behaviour penalty that is applied to the peer, assigned by the behaviour.
    behaviour_penality: float64
}

/// Stats assigned to peer for each topic.
struct TopicStats {
    /// True if the peer is in the mesh.
    in_mesh: bool,
    /// The time the peer was last GRAFTed; valid only when in the mesh.
    graft_time: Instant,
    /// The time the peer has been in the mesh.
    mesh_time: Duration,
    /// Number of first message deliveries.
    first_message_deliveries: usize,
    /// Number of message deliveries from the mesh.
    mesh_message_deliveries: usize,
    /// True if the peer has been in the mesh for enough time to active mesh message deliveries.
    mesh_message_deliveries_active: bool,
    /// Mesh rate failure penalty.
    mesh_failure_penalty: usize,
    /// Invalid message counter.
    invalid_message_deliveries: usize
}

struct DeliveryRecord {
    state: DeliveryState,
    first_seen: Instant,
    validated: Instant,
    peers: HashSet<PeerId>,
}

enum DeliveryState {
    /// Don't know (yet) if the message is valid.
    Unknown,
    /// The message is valid.
    Valid,
    /// The message is invalid.
    Invalid,
    /// Instructed by the validator to ignore the message.
    Ignored,
    /// Can't tell if the message is valid because validation was throttled.
    Throttled,
}

impl PeerScore {
    pub fn new(params: PeerScoreParams) -> Self {
        PeerScore {
            params,
            peer_stats: HashMap::new(),
            peer_ips: HashMap::new(),
            deliveries: HashMap::new(),
            msg_id: "PlaceHolder",
        }
    }


    /// Returns the score for a peer.
    pub fn score(&self, peer_id: &PeerId) -> f64 {
        let peer_stats = match self.peer_stats.get(peer_id) {
            Some(v) => v,
            None => return 0;
        };

        let mut score = 0;

        // topic scores
        for (topic, topic_stats) in peer_stats.iter() {
            // topic parameters
            if let Some(topic_params) = self.params.topics.get(topic) {
                // we are tracking the topic

                // the topic score
                let mut topic_score = 0;
               
                // P1: time in mesh
                if topic_stats.in_mesh {
                    let mut p1 = (topic_stats.mesh_time / topic_params.time_in_mesh_quantum);
                    if p1 > topic_params.time_in_mesh_cap {
                        // return the maximum
                        p1 = topic_params.time_in_mesh_cap;
                    } 
                    topic_score += p1 * topic_params.time_in_mesh_weight;
                }

                // P2: first message deliveries
                let p2 = topic_stats.first_message_deliveries;
                topic_score += p2 * topic_params.first_message_deliveries_weight;

                // P3: mesh message deliveries
                if topic_stats.mesh_message_deliveries_active {
                    if topic_stats.mesh_message_deliveries < topic_params.mesh_message_deliveries_threshold {
                        let deficit = topic_params.mesh_message_delivieries_threshold.saturating_sub(topic_params.mesh_message_deliveries);
                        let p3 = deficit * deficit;
                        topic_score += p3 * topic_params.mesh_message_deliveries_weight;
                    }
                }

		// P3b:
		// NOTE: the weight of P3b is negative (validated in TopicScoreParams.validate), so this detracts.
		let p3b = topic_stats.mesh_failure_penalty;
		topic_score += p3b * topic_params.mesh_failture_penalty_weight;

		// P4: invalid messages
		// NOTE: the weight of P4 is negative (validated in TopicScoreParams.validate), so this detracts.
		let p4 = (topic_stats.invalid_message_deliveries * topic_stats.invalid_message_deliveries);
		topic_score += p4 * topic_params.invalid_message_deliveries_weight;

		// update score, mixing with topic weight
		score += topic_score * topic_params.topic_weight;
            }
	}

	// apply the topic score cap, if any
	if self.params.topic_score_cap > 0 && score > self.params.topic_score_cap {
            score = self.params.topic_score_cap;
	}

	// P5: application-specific score
        //TODO: Add in
        /*
	let p5 = self.params.app_specific_score(peer_id);
	score += p5 * self.params.app_specific_weight;
        */

	// P6: IP collocation factor
	for ip in peer_stats.known_ips.iter() {
		if self.params.ip_colocation_factor_whitelist.get(ip).is_some() {
			continue;
		}

		// P6 has a cliff (ip_colocation_factor_threshold); it's only applied iff
		// at least that many peers are connected to us from that source IP
		// addr. It is quadratic, and the weight is negative (validated by
		// peer_score_params.validate()).
		if let Some(peers_in_ip) = self.peer_ips.get(ip).map(|peers| peers.len()) {
                    if peers_in_ip > self.params.ip_colocation_factor_threshold {
                            let surplus = (peers_in_ip.saturating_sub(self.params.ip_colocation_factor_threshold)) as f64;
                            let p6 = surplus * surplus;
                            score += p6 * self.params.ip_colocation_factor_weight;
                    }
                }
        }

	// P7: behavioural pattern penalty
	let p7 = peer_stats.behaviour_penalty * peer_stats.behaviour_penalty:
	score += p7 * self.params.behaviour_penalty_weight;
	score
    }

    pub fn add_penalty(&mut self, peer_id: &PeerId, count: usize) {
        if let Some(peer_stats) = self.peer_stats.get_mut(peer_id) {
            peer_stats.behaviour_penalty += count as f64;
        }
    }

    pub fn refreshScores(&mut self) {

	let now := Instant::now();
	for (peer, peer_stats) in self.peer_stats.iter() {
		if !peer_stats.connected {
			// has the retention period expired?
			if now > peer_stats.expire {
				// yes, throw it away (but clean up the IP tracking first)
				self.remove_ips(peer_id, peer_stats.ips);
                                // re address this, use retain or entry
				self.peer_stats.remove(peer_id);
			}

			// we don't decay retained scores, as the peer is not active.
			// this way the peer cannot reset a negative score by simply disconnecting and reconnecting,
			// unless the retention period has ellapsed.
			// similarly, a well behaved peer does not lose its score by getting disconnected.
			continue
		}

		for (topic, topic_stats) in peer_stats.topics.iter() {
			// the topic parameters
			if let Some(topic_params) = self.params.topics.get(topic) {

                            // decay counters
                            topic_stats.first_message_deliveries *= topic_params.first_message_deliveries_decay
                            if topic_stats.first_message_deliveries < ps.params.decay_to_zero {
                                    topic_stats.first_message_deliveries = 0
                            }
                            topic_stats.mesh_message_deliveries *= topic_params.mesh_message_deliveries_decay
                            if topic_stats.mesh_message_deliveries < ps.params.decay_to_zero {
                                    topic_stats.mesh_message_deliveries = 0
                            }
                            topic_stats.mesh_failure_penalty *= topic_params.mesh_failure_penalty_decay
                            if topic_stats.mesh_failure_penalty < ps.params.decay_to_zero {
                                    topic_stats.mesh_failure_penalty = 0
                            }
                            topic_stats.invalid_message_deliveries *= topic_params.invalid_message_deliveries_decay
                            if topic_stats.invalid_message_deliveries < ps.params.decay_to_zero {
                                    topic_stats.invalid_message_deliveries = 0
                            }
                            // update mesh time and activate mesh message delivery parameter if need be
                            if topic_stats.in_mesh {
                                    topic_stats.mesh_time = now.sub(topic_stats.graft_time)
                                    if topic_stats.mesh_time > topic_params.mesh_message_deliveries_activation {
                                            topic_stats.mesh_message_deliveries_active = true
                                    }
                            }

                        }
		}

		// decay P7 counter
		peer_stats.behaviour_penalty *= self.params.behaviour_penalty_decay
		if peer_stats.behaviour_penalty < self.params.decay_to_zero {
			peer_stats.behaviour_penalty = 0
		}
	}
    }
}
