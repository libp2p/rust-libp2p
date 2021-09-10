// Copyright 2020 Sigma Prime Pty Ltd.
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

use crate::TopicHash;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::time::Duration;

/// The default number of seconds for a decay interval.
const DEFAULT_DECAY_INTERVAL: u64 = 1;
/// The default rate to decay to 0.
const DEFAULT_DECAY_TO_ZERO: f64 = 0.1;

/// Computes the decay factor for a parameter, assuming the `decay_interval` is 1s
/// and that the value decays to zero if it drops below 0.01.
pub fn score_parameter_decay(decay: Duration) -> f64 {
    score_parameter_decay_with_base(
        decay,
        Duration::from_secs(DEFAULT_DECAY_INTERVAL),
        DEFAULT_DECAY_TO_ZERO,
    )
}

/// Computes the decay factor for a parameter using base as the `decay_interval`.
pub fn score_parameter_decay_with_base(decay: Duration, base: Duration, decay_to_zero: f64) -> f64 {
    // the decay is linear, so after n ticks the value is factor^n
    // so factor^n = decay_to_zero => factor = decay_to_zero^(1/n)
    let ticks = decay.as_secs_f64() / base.as_secs_f64();
    decay_to_zero.powf(1f64 / ticks)
}

#[derive(Debug, Clone)]
pub struct PeerScoreThresholds {
    /// The score threshold below which gossip propagation is suppressed;
    /// should be negative.
    pub gossip_threshold: f64,

    /// The score threshold below which we shouldn't publish when using flood
    /// publishing (also applies to fanout peers); should be negative and <= `gossip_threshold`.
    pub publish_threshold: f64,

    /// The score threshold below which message processing is suppressed altogether,
    /// implementing an effective graylist according to peer score; should be negative and
    /// <= `publish_threshold`.
    pub graylist_threshold: f64,

    /// The score threshold below which px will be ignored; this should be positive
    /// and limited to scores attainable by bootstrappers and other trusted nodes.
    pub accept_px_threshold: f64,

    /// The median mesh score threshold before triggering opportunistic
    /// grafting; this should have a small positive value.
    pub opportunistic_graft_threshold: f64,
}

impl Default for PeerScoreThresholds {
    fn default() -> Self {
        PeerScoreThresholds {
            gossip_threshold: -10.0,
            publish_threshold: -50.0,
            graylist_threshold: -80.0,
            accept_px_threshold: 10.0,
            opportunistic_graft_threshold: 20.0,
        }
    }
}

impl PeerScoreThresholds {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.gossip_threshold > 0f64 {
            return Err("invalid gossip threshold; it must be <= 0");
        }
        if self.publish_threshold > 0f64 || self.publish_threshold > self.gossip_threshold {
            return Err("Invalid publish threshold; it must be <= 0 and <= gossip threshold");
        }
        if self.graylist_threshold > 0f64 || self.graylist_threshold > self.publish_threshold {
            return Err("Invalid graylist threshold; it must be <= 0 and <= publish threshold");
        }
        if self.accept_px_threshold < 0f64 {
            return Err("Invalid accept px threshold; it must be >= 0");
        }
        if self.opportunistic_graft_threshold < 0f64 {
            return Err("Invalid opportunistic grafting threshold; it must be >= 0");
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PeerScoreParams {
    /// Score parameters per topic.
    pub topics: HashMap<TopicHash, TopicScoreParams>,

    /// Aggregate topic score cap; this limits the total contribution of topics towards a positive
    /// score. It must be positive (or 0 for no cap).
    pub topic_score_cap: f64,

    /// P5: Application-specific peer scoring
    pub app_specific_weight: f64,

    ///  P6: IP-colocation factor.
    ///  The parameter has an associated counter which counts the number of peers with the same IP.
    ///  If the number of peers in the same IP exceeds `ip_colocation_factor_threshold, then the value
    ///  is the square of the difference, ie `(peers_in_same_ip - ip_colocation_threshold)^2`.
    ///  If the number of peers in the same IP is less than the threshold, then the value is 0.
    ///  The weight of the parameter MUST be negative, unless you want to disable for testing.
    ///  Note: In order to simulate many IPs in a manageable manner when testing, you can set the weight to 0
    ///        thus disabling the IP colocation penalty.
    pub ip_colocation_factor_weight: f64,
    pub ip_colocation_factor_threshold: f64,
    pub ip_colocation_factor_whitelist: HashSet<IpAddr>,

    ///  P7: behavioural pattern penalties.
    ///  This parameter has an associated counter which tracks misbehaviour as detected by the
    ///  router. The router currently applies penalties for the following behaviors:
    ///  - attempting to re-graft before the prune backoff time has elapsed.
    ///  - not following up in IWANT requests for messages advertised with IHAVE.
    ///
    ///  The value of the parameter is the square of the counter over the threshold, which decays
    ///  with BehaviourPenaltyDecay.
    ///  The weight of the parameter MUST be negative (or zero to disable).
    pub behaviour_penalty_weight: f64,
    pub behaviour_penalty_threshold: f64,
    pub behaviour_penalty_decay: f64,

    /// The decay interval for parameter counters.
    pub decay_interval: Duration,

    /// Counter value below which it is considered 0.
    pub decay_to_zero: f64,

    /// Time to remember counters for a disconnected peer.
    pub retain_score: Duration,
}

impl Default for PeerScoreParams {
    fn default() -> Self {
        PeerScoreParams {
            topics: HashMap::new(),
            topic_score_cap: 3600.0,
            app_specific_weight: 10.0,
            ip_colocation_factor_weight: -5.0,
            ip_colocation_factor_threshold: 10.0,
            ip_colocation_factor_whitelist: HashSet::new(),
            behaviour_penalty_weight: -10.0,
            behaviour_penalty_threshold: 0.0,
            behaviour_penalty_decay: 0.2,
            decay_interval: Duration::from_secs(DEFAULT_DECAY_INTERVAL),
            decay_to_zero: DEFAULT_DECAY_TO_ZERO,
            retain_score: Duration::from_secs(3600),
        }
    }
}

/// Peer score parameter validation
impl PeerScoreParams {
    pub fn validate(&self) -> Result<(), String> {
        for (topic, params) in self.topics.iter() {
            if let Err(e) = params.validate() {
                return Err(format!(
                    "Invalid score parameters for topic {}: {}",
                    topic, e
                ));
            }
        }

        // check that the topic score is 0 or something positive
        if self.topic_score_cap < 0f64 {
            return Err("Invalid topic score cap; must be positive (or 0 for no cap)".into());
        }

        // check the IP colocation factor
        if self.ip_colocation_factor_weight > 0f64 {
            return Err(
                "Invalid ip_colocation_factor_weight; must be negative (or 0 to disable)".into(),
            );
        }
        if self.ip_colocation_factor_weight != 0f64 && self.ip_colocation_factor_threshold < 1f64 {
            return Err("Invalid ip_colocation_factor_threshold; must be at least 1".into());
        }

        // check the behaviour penalty
        if self.behaviour_penalty_weight > 0f64 {
            return Err(
                "Invalid behaviour_penalty_weight; must be negative (or 0 to disable)".into(),
            );
        }
        if self.behaviour_penalty_weight != 0f64
            && (self.behaviour_penalty_decay <= 0f64 || self.behaviour_penalty_decay >= 1f64)
        {
            return Err("invalid behaviour_penalty_decay; must be between 0 and 1".into());
        }

        if self.behaviour_penalty_threshold < 0f64 {
            return Err("invalid behaviour_penalty_threshold; must be >= 0".into());
        }

        // check the decay parameters
        if self.decay_interval < Duration::from_secs(1) {
            return Err("Invalid decay_interval; must be at least 1s".into());
        }
        if self.decay_to_zero <= 0f64 || self.decay_to_zero >= 1f64 {
            return Err("Invalid decay_to_zero; must be between 0 and 1".into());
        }

        // no need to check the score retention; a value of 0 means that we don't retain scores
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct TopicScoreParams {
    /// The weight of the topic.
    pub topic_weight: f64,

    ///  P1: time in the mesh
    ///  This is the time the peer has been grafted in the mesh.
    ///  The value of of the parameter is the `time/time_in_mesh_quantum`, capped by `time_in_mesh_cap`
    ///  The weight of the parameter must be positive (or zero to disable).
    pub time_in_mesh_weight: f64,
    pub time_in_mesh_quantum: Duration,
    pub time_in_mesh_cap: f64,

    ///  P2: first message deliveries
    ///  This is the number of message deliveries in the topic.
    ///  The value of the parameter is a counter, decaying with `first_message_deliveries_decay`, and capped
    ///  by `first_message_deliveries_cap`.
    ///  The weight of the parameter MUST be positive (or zero to disable).
    pub first_message_deliveries_weight: f64,
    pub first_message_deliveries_decay: f64,
    pub first_message_deliveries_cap: f64,

    ///  P3: mesh message deliveries
    ///  This is the number of message deliveries in the mesh, within the
    ///  `mesh_message_deliveries_window` of message validation; deliveries during validation also
    ///  count and are retroactively applied when validation succeeds.
    ///  This window accounts for the minimum time before a hostile mesh peer trying to game the
    ///  score could replay back a valid message we just sent them.
    ///  It effectively tracks first and near-first deliveries, ie a message seen from a mesh peer
    ///  before we have forwarded it to them.
    ///  The parameter has an associated counter, decaying with `mesh_message_deliveries_decay`.
    ///  If the counter exceeds the threshold, its value is 0.
    ///  If the counter is below the `mesh_message_deliveries_threshold`, the value is the square of
    ///  the deficit, ie (`message_deliveries_threshold - counter)^2`
    ///  The penalty is only activated after `mesh_message_deliveries_activation` time in the mesh.
    ///  The weight of the parameter MUST be negative (or zero to disable).
    pub mesh_message_deliveries_weight: f64,
    pub mesh_message_deliveries_decay: f64,
    pub mesh_message_deliveries_cap: f64,
    pub mesh_message_deliveries_threshold: f64,
    pub mesh_message_deliveries_window: Duration,
    pub mesh_message_deliveries_activation: Duration,

    ///  P3b: sticky mesh propagation failures
    ///  This is a sticky penalty that applies when a peer gets pruned from the mesh with an active
    ///  mesh message delivery penalty.
    ///  The weight of the parameter MUST be negative (or zero to disable)
    pub mesh_failure_penalty_weight: f64,
    pub mesh_failure_penalty_decay: f64,

    ///  P4: invalid messages
    ///  This is the number of invalid messages in the topic.
    ///  The value of the parameter is the square of the counter, decaying with
    ///  `invalid_message_deliveries_decay`.
    ///  The weight of the parameter MUST be negative (or zero to disable).
    pub invalid_message_deliveries_weight: f64,
    pub invalid_message_deliveries_decay: f64,
}

/// NOTE: The topic score parameters are very network specific.
///       For any production system, these values should be manually set.
impl Default for TopicScoreParams {
    fn default() -> Self {
        TopicScoreParams {
            topic_weight: 0.5,
            // P1
            time_in_mesh_weight: 1.0,
            time_in_mesh_quantum: Duration::from_millis(1),
            time_in_mesh_cap: 3600.0,
            // P2
            first_message_deliveries_weight: 1.0,
            first_message_deliveries_decay: 0.5,
            first_message_deliveries_cap: 2000.0,
            // P3
            mesh_message_deliveries_weight: -1.0,
            mesh_message_deliveries_decay: 0.5,
            mesh_message_deliveries_cap: 100.0,
            mesh_message_deliveries_threshold: 20.0,
            mesh_message_deliveries_window: Duration::from_millis(10),
            mesh_message_deliveries_activation: Duration::from_secs(5),
            // P3b
            mesh_failure_penalty_weight: -1.0,
            mesh_failure_penalty_decay: 0.5,
            // P4
            invalid_message_deliveries_weight: -1.0,
            invalid_message_deliveries_decay: 0.3,
        }
    }
}

impl TopicScoreParams {
    pub fn validate(&self) -> Result<(), &'static str> {
        // make sure we have a sane topic weight
        if self.topic_weight < 0f64 {
            return Err("invalid topic weight; must be >= 0");
        }

        if self.time_in_mesh_quantum == Duration::from_secs(0) {
            return Err("Invalid time_in_mesh_quantum; must be non zero");
        }
        if self.time_in_mesh_weight < 0f64 {
            return Err("Invalid time_in_mesh_weight; must be positive (or 0 to disable)");
        }
        if self.time_in_mesh_weight != 0f64 && self.time_in_mesh_cap <= 0f64 {
            return Err("Invalid time_in_mesh_cap must be positive");
        }

        if self.first_message_deliveries_weight < 0f64 {
            return Err(
                "Invalid first_message_deliveries_weight; must be positive (or 0 to disable)",
            );
        }
        if self.first_message_deliveries_weight != 0f64
            && (self.first_message_deliveries_decay <= 0f64
                || self.first_message_deliveries_decay >= 1f64)
        {
            return Err("Invalid first_message_deliveries_decay; must be between 0 and 1");
        }
        if self.first_message_deliveries_weight != 0f64 && self.first_message_deliveries_cap <= 0f64
        {
            return Err("Invalid first_message_deliveries_cap must be positive");
        }

        if self.mesh_message_deliveries_weight > 0f64 {
            return Err(
                "Invalid mesh_message_deliveries_weight; must be negative (or 0 to disable)",
            );
        }
        if self.mesh_message_deliveries_weight != 0f64
            && (self.mesh_message_deliveries_decay <= 0f64
                || self.mesh_message_deliveries_decay >= 1f64)
        {
            return Err("Invalid mesh_message_deliveries_decay; must be between 0 and 1");
        }
        if self.mesh_message_deliveries_weight != 0f64 && self.mesh_message_deliveries_cap <= 0f64 {
            return Err("Invalid mesh_message_deliveries_cap must be positive");
        }
        if self.mesh_message_deliveries_weight != 0f64
            && self.mesh_message_deliveries_threshold <= 0f64
        {
            return Err("Invalid mesh_message_deliveries_threshold; must be positive");
        }
        if self.mesh_message_deliveries_weight != 0f64
            && self.mesh_message_deliveries_activation < Duration::from_secs(1)
        {
            return Err("Invalid mesh_message_deliveries_activation; must be at least 1s");
        }

        // check P3b
        if self.mesh_failure_penalty_weight > 0f64 {
            return Err("Invalid mesh_failure_penalty_weight; must be negative (or 0 to disable)");
        }
        if self.mesh_failure_penalty_weight != 0f64
            && (self.mesh_failure_penalty_decay <= 0f64 || self.mesh_failure_penalty_decay >= 1f64)
        {
            return Err("Invalid mesh_failure_penalty_decay; must be between 0 and 1");
        }

        // check P4
        if self.invalid_message_deliveries_weight > 0f64 {
            return Err(
                "Invalid invalid_message_deliveries_weight; must be negative (or 0 to disable)",
            );
        }
        if self.invalid_message_deliveries_decay <= 0f64
            || self.invalid_message_deliveries_decay >= 1f64
        {
            return Err("Invalid invalid_message_deliveries_decay; must be between 0 and 1");
        }
        Ok(())
    }
}
