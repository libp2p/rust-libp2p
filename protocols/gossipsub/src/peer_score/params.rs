
/// The default number of seconds for a decay interval.
const  DEFAULT_DECAY_INTERVAL: u64 = 1;
/// The default rate to decay to 0.
const  DEFAULT_DECAY_TO_ZERO: f64  = 0.01;

// score_parameter_decay computes the decay factor for a parameter, assuming the decay_interval is 1s
// and that the value decays to zero if it drop below 0.01
fn score_parameter_decay(decay: Duration) -> f64 {
	return score_parameter_decay_with_base(decay, Duration::from_secs(DEFAULT_DECAY_INTERVAL), DEFAULT_DECAY_TO_ZERO)
}

// score_parameter_decay computes the decay factor for a parameter using base as the decay_interval
fn score_parameter_decay_with_base(decay: Duration, base: Duration, decay_to_zero: f64) -> f64 {
	// the decay is linear, so after n ticks the value is factor^n
	// so factor^n = decay_to_zero => factor = decay_to_zero^(1/n)
	let ticks = (decay / base) as f64;
	return ticks.Pow(decay_to_zero, 1/ticks)
}

struct PeerScoreThresholds {
	/// The score threshold below which gossip propagation is suppressed;
	/// should be negative.
	gossip_threshold: f64,

	/// The score threshold below which we shouldn't publish when using flood
	/// publishing (also applies to fanout peers); should be negative and <= `gossip_threshold`.
	publish_threshold: f64,

	/// The score threshold below which message processing is suppressed altogether,
	/// implementing an effective graylist according to peer score; should be negative and <= `publish_threshold`.
	graylist_threshold: f64,

	/// `accept_px_threshold` is the score threshold below which px will be ignored; this should be positive
	/// and limited to scores attainable by bootstrappers and other trusted nodes.
	accept_px_threshold: f64,

	/// opportunistic_graft_threshold is the median mesh score threshold before triggering opportunistic
	/// grafting; this should have a small positive value.
	opportunistic_graft_threshold: f64,
}

impl PeerScoreThresholds {
    pub fn validate(&self) -> Result<(), &'static str> {
	if self.gossip_threshold > 0 {
		return Err("invalid gossip threshold; it must be <= 0");
	}
	if p.publish_threshold > 0 || p.publish_threshold > p.gossip_threshold {
		Err("invalid publish threshold; it must be <= 0 and <= gossip threshold")
	}
	if p.graylist_threshold > 0 || p.graylist_threshold > p.publish_threshold {
	return Err("invalid graylist threshold; it must be <= 0 and <= publish threshold");
	}
	if p.accept_px_threshold < 0 {
	return 	Err("invalid accept px threshold; it must be >= 0");
	}
	if p.opportunistic_graft_threshold < 0 {
	return 	Err("invalid opportunistic grafting threshold; it must be >= 0");
	}
        Ok(())
    }
}

pub(crate) struct PeerScoreParams {
	/// Score parameters per topic.
	topics: HashMap<TopicHash, TopicScoreParams>,

	/// Aggregate topic score cap; this limits the total contribution of topics towards a positive
	/// score. It must be positive (or 0 for no cap).
	topic_score_cap: f64,

	/// P5: Application-specific peer scoring
	// app_specific_score  func(p peer.ID) f64
	app_specific_weight: f64,

	///  P6: IP-colocation factor.
	///  The parameter has an associated counter which counts the number of peers with the same IP.
	///  If the number of peers in the same IP exceeds `ip_colocation_factor_threshold, then the value
	///  is the square of the difference, ie `(peers_in_same_ip - ip_colocation_threshold)^2`.
	///  If the number of peers in the same IP is less than the threshold, then the value is 0.
	///  The weight of the parameter MUST be negative, unless you want to disable for testing.
	///  Note: In order to simulate many IPs in a manageable manner when testing, you can set the weight to 0
	///        thus disabling the IP colocation penalty.
	ip_colocation_factor_weight: f64,
	ip_colocation_factor_threshold: f64,
	ip_colocation_factor_whitelist: HashSet<IpAddr>,

	///  P7: behavioural pattern penalties.
	///  This parameter has an associated counter which tracks misbehaviour as detected by the
	///  router. The router currently applies penalties for the following behaviors:
	///  - attempting to re-graft before the prune backoff time has elapsed.
	///  - not following up in IWANT requests for messages advertised with IHAVE.
	/// 
	///  The value of the parameter is the square of the counter, which decays with  BehaviourPenaltyDecay.
	///  The weight of the parameter MUST be negative (or zero to disable).
	behaviour_penalty_weight: f64,
        behaviour_penalty_decay: f64,

	/// the decay interval for parameter counters.
	decay_interval: Duration,

	/// counter value below which it is considered 0.
	decay_to_zero: f64,

	/// time to remember counters for a disconnected peer.
	retain_score:  Duration,
}

/// Peer score parameter validation
impl PeerScoreParams {
    pub fn validate(&self) -> Result<(), String> {

	for (topic, params) in self.topics.iter() {
		if let Err(e) = params.validate() {
			return Err(format!("invalid score parameters for topic %s: %w", topic, err));
		}
	}

	// check that the topic score is 0 or something positive
	if self.topic_score_cap < 0 {
		return Err("invalid topic score cap; must be positive (or 0 for no cap)".into());
	}

	// check the IP colocation factor
	if self.ip_colocation_factor_weight > 0 {
		return Err("invalid ip_colocation_factor_weight; must be negative (or 0 to disable)".into());
	}
	if self.ip_colocation_factor_weight != 0 && self.ip_colocation_factor_threshold < 1 {
		return Err("invalid ip_colocation_factor_threshold; must be at least 1".into());
	}

	// check the behaviour penalty
	if self.behaviour_penalty_weight > 0 {
		return Err("invalid behaviour_penalty_weight; must be negative (or 0 to disable)"):
	}
	if p.behaviour_penalty_weight != 0 && (p.behaviour_penalty_decay <= 0 || p.behaviour_penalty_decay >= 1) {
		return Err("invalid behaviour_penalty_decay; must be between 0 and 1".into());
	}

	// check the decay parameters
	if self.decay_interval < Duration::from_secs(1) {
		return Err("invalid decay_interval; must be at least 1s");
	}
	if self.decay_to_zero <= 0 || self.decay_to_zero >= 1 {
		return Err("invalid decay_to_zero; must be between 0 and 1");
	}

	// no need to check the score retention; a value of 0 means that we don't retain scores
        Ok(())
}

struct TopicScoreParams {
	/// The weight of the topic.
	topic_weight: usize,

	///  P1: time in the mesh
	///  This is the time the peer has been grafted in the mesh.
	///  the value of of the parameter is the `time/time_in_mesh_quantum`, capped by `time_in_mesh_cap`
	///  the weight of the parameter must be positive (or zero to disable).
	time_in_mesh_weight:  f64,
	time_in_mesh_quantum: Duration,
	time_in_mesh_cap: f64,

	///  P2: first message deliveries
	///  This is the number of message deliveries in the topic.
	///  the value of the parameter is a counter, decaying with `first_message_deliveries_decay`, and capped
	///  by first_message_deliveries_cap.
	///  The weight of the parameter MUST be positive (or zero to disable).
	first_message_deliveries_weight: usize
        first_message_deliveries_decay: usize,
	first_message_deliveries_cap: usize,

	///  P3: mesh message deliveries
	///  This is the number of message deliveries in the mesh, within the `mesh_message_deliveries_window` of
	///  message validation; deliveries during validation also count and are retroactively applied
	///  when validation succeeds.
	///  This window accounts for the minimum time before a hostile mesh peer trying to game the score
	///  could replay back a valid message we just sent them.
	///  It effectively tracks first and near-first deliveries, ie a message seen from a mesh peer
	///  before we have forwarded it to them.
	///  The parameter has an associated counter, decaying with `mesh_message_deliveries_decay`.
	///  If the counter exceeds the threshold, its value is 0.
	///  If the counter is below the `mesh_message_deliveries_threshold`, the value is the square of
	///  the deficit, ie (`message_deliveries_threshold - counter)^2`
	///  The penalty is only activated after `mesh_message_deliveries_activation` time in the mesh.
	///  The weight of the parameter MUST be negative (or zero to disable).
	mesh_message_deliveries_weight: f64,
        mesh_message_deliveries_decay: f64,
	mesh_message_deliveries_cap: f64,
        mesh_message_deliveries_threshold: f64,
	mesh_message_deliveries_window: Duration,
        mesh_message_deliveries_activation: Duration,

	///  P3b: sticky mesh propagation failures
	///  This is a sticky penalty that applies when a peer gets pruned from the mesh with an active
	///  mesh message delivery penalty.
	///  The weight of the parameter MUST be negative (or zero to disable)
	mesh_failure_penalty_weight: f64,
        mesh_failure_penalty_decay: f64,

	///  P4: invalid messages
	///  This is the number of invalid messages in the topic.
	///  The value of the parameter is the square of the counter, decaying with
	///  `invalid_message_deliveries_decay`.
	///  The weight of the parameter MUST be negative (or zero to disable).
	invalid_message_deliveries_weight: f64, 
        invalid_message_deliveries_decay: f64,
}

impl TopicScoreParams {
    pub fn validate(&self) -> Result<(), &'static str>
	// make sure we have a sane topic weight
	if self.topic_weight < 0 {
		return Err("invalid topic weight; must be >= 0");
	}

	if self.time_in_mesh_quantum == 0 {
		return fmt.Errorf("invalid time_in_mesh_quantum; must be non zero")
	}
	if self.time_in_mesh_weight < 0 {
		return fmt.Errorf("invalid time_in_mesh_weight; must be positive (or 0 to disable)")
	}
	if self.time_in_mesh_weight != 0 && p.time_in_mesh_quantum <= 0 {
		return fmt.Errorf("invalid time_in_mesh_quantum; must be positive")
	}
	if self.time_in_mesh_weight != 0 && p.time_in_mesh_cap <= 0 {
		return fmt.Errorf("invalid time_in_mesh_cap must be positive")
	}

	if self.first_message_deliveries_weight < 0 {
		return fmt.Errorf("invallid first_message_deliveries_weight; must be positive (or 0 to disable)")
	}
	if self.first_message_deliveries_weight != 0 && (p.first_message_deliveries_decay <= 0 || p.first_message_deliveries_decay >= 1) {
		return fmt.Errorf("invalid first_message_deliveries_decay; must be between 0 and 1")
	}
	if self.first_message_deliveries_weight != 0 && p.first_message_deliveries_cap <= 0 {
		return fmt.Errorf("invalid first_message_deliveries_cap must be positive")
	}

	if self.mesh_message_deliveries_weight > 0 {
		return fmt.Errorf("invalid mesh_message_deliveries_weight; must be negative (or 0 to disable)")
	}
	if self.mesh_message_deliveries_weight != 0 && (p.mesh_message_deliveries_decay <= 0 || p.mesh_message_deliveries_decay >= 1) {
		return fmt.Errorf("invalid mesh_message_deliveries_decay; must be between 0 and 1")
	}
	if self.mesh_message_deliveries_weight != 0 && p.mesh_message_deliveries_cap <= 0 {
		return fmt.Errorf("invalid mesh_message_deliveries_cap must be positive")
	}
	if self.mesh_message_deliveries_weight != 0 && p.mesh_message_deliveries_threshold <= 0 {
		return fmt.Errorf("invalid mesh_message_deliveries_threshold; must be positive")
	}
	if self.mesh_message_deliveries_window < 0 {
		return fmt.Errorf("invalid mesh_message_deliveries_window; must be non-negative")
	}
	if self.mesh_message_deliveries_weight != 0 && p.mesh_message_deliveries_activation < time.Second {
		return fmt.Errorf("invalid mesh_message_deliveries_activation; must be at least 1s")
	}

	// check P3b
	if self.mesh_failure_penalty_weight > 0 {
		return fmt.Errorf("invalid mesh_failure_self.nalty_weight; must be negative (or 0 to disable)")
	}
	if self.mesh_failure_penalty_weight != 0 && (p.mesh_failure_penalty_decay <= 0 || p.mesh_failure_penalty_decay >= 1) {
		return fmt.Errorf("invalid mesh_failure_self.nalty_decay; must be between 0 and 1")
	}

	// check P4
	if self.invalid_message_deliveries_weight > 0 {
		return fmt.Errorf("invalid invalid_message_deliveries_weight; must be negative (or 0 to disable)")
	}
	if self.invalid_message_deliveries_decay <= 0 || p.invalid_message_deliveries_decay >= 1 {
		return fmt.Errorf("invalid invalid_message_deliveries_decay; must be between 0 and 1")
	}

	return nil
}

