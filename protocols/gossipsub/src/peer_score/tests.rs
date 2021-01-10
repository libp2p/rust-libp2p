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

/// A collection of unit tests mostly ported from the go implementation.
use super::*;

use crate::types::RawGossipsubMessage;
use crate::{GossipsubMessage, IdentTopic as Topic};

// estimates a value within variance
fn within_variance(value: f64, expected: f64, variance: f64) -> bool {
    if expected >= 0.0 {
        return value > expected * (1.0 - variance) && value < expected * (1.0 + variance);
    }
    return value > expected * (1.0 + variance) && value < expected * (1.0 - variance);
}

// generates a random gossipsub message with sequence number i
fn make_test_message(seq: u64) -> (MessageId, RawGossipsubMessage) {
    let raw_message = RawGossipsubMessage {
        source: Some(PeerId::random()),
        data: vec![12, 34, 56],
        sequence_number: Some(seq),
        topic: Topic::new("test").hash(),
        signature: None,
        key: None,
        validated: true,
    };

    let message = GossipsubMessage {
        source: raw_message.source.clone(),
        data: raw_message.data.clone(),
        sequence_number: raw_message.sequence_number,
        topic: raw_message.topic.clone(),
    };

    let id = default_message_id()(&message);
    (id, raw_message)
}

fn default_message_id() -> fn(&GossipsubMessage) -> MessageId {
    |message| {
        // default message id is: source + sequence number
        // NOTE: If either the peer_id or source is not provided, we set to 0;
        let mut source_string = if let Some(peer_id) = message.source.as_ref() {
            peer_id.to_base58()
        } else {
            PeerId::from_bytes(&vec![0, 1, 0])
                .expect("Valid peer id")
                .to_base58()
        };
        source_string.push_str(&message.sequence_number.unwrap_or_default().to_string());
        MessageId::from(source_string)
    }
}

#[test]
fn test_score_time_in_mesh() {
    // Create parameters with reasonable default values
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let mut params = PeerScoreParams::default();
    params.topic_score_cap = 1000.0;

    let mut topic_params = TopicScoreParams::default();
    topic_params.topic_weight = 0.5;
    topic_params.time_in_mesh_weight = 1.0;
    topic_params.time_in_mesh_quantum = Duration::from_millis(1);
    topic_params.time_in_mesh_cap = 3600.0;

    params.topics.insert(topic_hash, topic_params.clone());

    let peer_id = PeerId::random();

    let mut peer_score = PeerScore::new(params);
    // Peer score should start at 0
    peer_score.add_peer(peer_id.clone());

    let score = peer_score.score(&peer_id);
    assert!(
        score == 0.0,
        "expected score to start at zero. Score found: {}",
        score
    );

    // The time in mesh depends on how long the peer has been grafted
    peer_score.graft(&peer_id, topic);
    let elapsed = topic_params.time_in_mesh_quantum * 200;
    std::thread::sleep(elapsed);
    peer_score.refresh_scores();

    let score = peer_score.score(&peer_id);
    let expected = topic_params.topic_weight
        * topic_params.time_in_mesh_weight
        * (elapsed.as_millis() / topic_params.time_in_mesh_quantum.as_millis()) as f64;
    assert!(
        score >= expected,
        "The score: {} should be greater than or equal to: {}",
        score,
        expected
    );
}

#[test]
fn test_score_time_in_mesh_cap() {
    // Create parameters with reasonable default values
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let mut params = PeerScoreParams::default();

    let mut topic_params = TopicScoreParams::default();
    topic_params.topic_weight = 0.5;
    topic_params.time_in_mesh_weight = 1.0;
    topic_params.time_in_mesh_quantum = Duration::from_millis(1);
    topic_params.time_in_mesh_cap = 10.0;

    params.topics.insert(topic_hash, topic_params.clone());

    let peer_id = PeerId::random();

    let mut peer_score = PeerScore::new(params);
    // Peer score should start at 0
    peer_score.add_peer(peer_id.clone());

    let score = peer_score.score(&peer_id);
    assert!(
        score == 0.0,
        "expected score to start at zero. Score found: {}",
        score
    );

    // The time in mesh depends on how long the peer has been grafted
    peer_score.graft(&peer_id, topic);
    let elapsed = topic_params.time_in_mesh_quantum * 40;
    std::thread::sleep(elapsed);
    peer_score.refresh_scores();

    let score = peer_score.score(&peer_id);
    let expected = topic_params.topic_weight
        * topic_params.time_in_mesh_weight
        * topic_params.time_in_mesh_cap;
    let variance = 0.5;
    assert!(
        within_variance(score, expected, variance),
        "The score: {} should be within {} of {}",
        score,
        score * variance,
        expected
    );
}

#[test]
fn test_score_first_message_deliveries() {
    // Create parameters with reasonable default values
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let mut params = PeerScoreParams::default();

    let mut topic_params = TopicScoreParams::default();
    topic_params.topic_weight = 1.0;
    topic_params.first_message_deliveries_weight = 1.0;
    topic_params.first_message_deliveries_decay = 1.0;
    topic_params.first_message_deliveries_cap = 2000.0;
    topic_params.time_in_mesh_weight = 0.0;

    params.topics.insert(topic_hash, topic_params.clone());

    let peer_id = PeerId::random();

    let mut peer_score = PeerScore::new(params);
    // Peer score should start at 0
    peer_score.add_peer(peer_id.clone());
    peer_score.graft(&peer_id, topic);

    // deliver a bunch of messages from the peer
    let messages = 100;
    for seq in 0..messages {
        let (id, msg) = make_test_message(seq);
        peer_score.validate_message(&peer_id, &id, &msg.topic);
        peer_score.deliver_message(&peer_id, &id, &msg.topic);
    }

    peer_score.refresh_scores();

    let score = peer_score.score(&peer_id);
    let expected =
        topic_params.topic_weight * topic_params.first_message_deliveries_weight * messages as f64;
    assert!(
        score == expected,
        "The score: {} should be {}",
        score,
        expected
    );
}

#[test]
fn test_score_first_message_deliveries_cap() {
    // Create parameters with reasonable default values
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let mut params = PeerScoreParams::default();

    let mut topic_params = TopicScoreParams::default();
    topic_params.topic_weight = 1.0;
    topic_params.first_message_deliveries_weight = 1.0;
    topic_params.first_message_deliveries_decay = 1.0; // test without decay
    topic_params.first_message_deliveries_cap = 50.0;
    topic_params.time_in_mesh_weight = 0.0;

    params.topics.insert(topic_hash, topic_params.clone());

    let peer_id = PeerId::random();

    let mut peer_score = PeerScore::new(params);
    // Peer score should start at 0
    peer_score.add_peer(peer_id.clone());
    peer_score.graft(&peer_id, topic);

    // deliver a bunch of messages from the peer
    let messages = 100;
    for seq in 0..messages {
        let (id, msg) = make_test_message(seq);
        peer_score.validate_message(&peer_id, &id, &msg.topic);
        peer_score.deliver_message(&peer_id, &id, &msg.topic);
    }

    peer_score.refresh_scores();
    let score = peer_score.score(&peer_id);
    let expected = topic_params.topic_weight
        * topic_params.first_message_deliveries_weight
        * topic_params.first_message_deliveries_cap;
    assert!(
        score == expected,
        "The score: {} should be {}",
        score,
        expected
    );
}

#[test]
fn test_score_first_message_deliveries_decay() {
    // Create parameters with reasonable default values
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let mut params = PeerScoreParams::default();

    let mut topic_params = TopicScoreParams::default();
    topic_params.topic_weight = 1.0;
    topic_params.first_message_deliveries_weight = 1.0;
    topic_params.first_message_deliveries_decay = 0.9; // decay 10% per decay interval
    topic_params.first_message_deliveries_cap = 2000.0;
    topic_params.time_in_mesh_weight = 0.0;

    params.topics.insert(topic_hash, topic_params.clone());
    let peer_id = PeerId::random();
    let mut peer_score = PeerScore::new(params);
    peer_score.add_peer(peer_id.clone());
    peer_score.graft(&peer_id, topic);

    // deliver a bunch of messages from the peer
    let messages = 100;
    for seq in 0..messages {
        let (id, msg) = make_test_message(seq);
        peer_score.validate_message(&peer_id, &id, &msg.topic);
        peer_score.deliver_message(&peer_id, &id, &msg.topic);
    }

    peer_score.refresh_scores();
    let score = peer_score.score(&peer_id);
    let mut expected = topic_params.topic_weight
        * topic_params.first_message_deliveries_weight
        * topic_params.first_message_deliveries_decay
        * messages as f64;
    assert!(
        score == expected,
        "The score: {} should be {}",
        score,
        expected
    );

    // refreshing the scores applies the decay param
    let decay_intervals = 10;
    for _ in 0..decay_intervals {
        peer_score.refresh_scores();
        expected *= topic_params.first_message_deliveries_decay;
    }
    let score = peer_score.score(&peer_id);
    assert!(
        score == expected,
        "The score: {} should be {}",
        score,
        expected
    );
}

#[test]
fn test_score_mesh_message_deliveries() {
    // Create parameters with reasonable default values
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let mut params = PeerScoreParams::default();

    let mut topic_params = TopicScoreParams::default();
    topic_params.topic_weight = 1.0;
    topic_params.mesh_message_deliveries_weight = -1.0;
    topic_params.mesh_message_deliveries_activation = Duration::from_secs(1);
    topic_params.mesh_message_deliveries_window = Duration::from_millis(10);
    topic_params.mesh_message_deliveries_threshold = 20.0;
    topic_params.mesh_message_deliveries_cap = 100.0;
    topic_params.mesh_message_deliveries_decay = 1.0;
    topic_params.first_message_deliveries_weight = 0.0;
    topic_params.time_in_mesh_weight = 0.0;
    topic_params.mesh_failure_penalty_weight = 0.0;

    params.topics.insert(topic_hash, topic_params.clone());
    let mut peer_score = PeerScore::new(params);

    // peer A always delivers the message first.
    // peer B delivers next (within the delivery window).
    // peer C delivers outside the delivery window.
    // we expect peers A and B to have a score of zero, since all other parameter weights are zero.
    // Peer C should have a negative score.
    let peer_id_a = PeerId::random();
    let peer_id_b = PeerId::random();
    let peer_id_c = PeerId::random();

    let peers = vec![peer_id_a.clone(), peer_id_b.clone(), peer_id_c.clone()];

    for peer_id in &peers {
        peer_score.add_peer(peer_id.clone());
        peer_score.graft(&peer_id, topic.clone());
    }

    // assert that nobody has been penalized yet for not delivering messages before activation time
    peer_score.refresh_scores();
    for peer_id in &peers {
        let score = peer_score.score(peer_id);
        assert!(
            score >= 0.0,
            "expected no mesh delivery penalty before activation time, got score {}",
            score
        );
    }

    // wait for the activation time to kick in
    std::thread::sleep(topic_params.mesh_message_deliveries_activation);

    // deliver a bunch of messages from peer A, with duplicates within the window from peer B,
    // and duplicates outside the window from peer C.
    let messages = 100;
    let mut messages_to_send = Vec::new();
    for seq in 0..messages {
        let (id, msg) = make_test_message(seq);
        peer_score.validate_message(&peer_id_a, &id, &msg.topic);
        peer_score.deliver_message(&peer_id_a, &id, &msg.topic);

        peer_score.duplicated_message(&peer_id_b, &id, &msg.topic);
        messages_to_send.push((id, msg));
    }

    std::thread::sleep(topic_params.mesh_message_deliveries_window + Duration::from_millis(20));

    for (id, msg) in messages_to_send {
        peer_score.duplicated_message(&peer_id_c, &id, &msg.topic);
    }

    peer_score.refresh_scores();
    let score_a = peer_score.score(&peer_id_a);
    let score_b = peer_score.score(&peer_id_b);
    let score_c = peer_score.score(&peer_id_c);

    assert!(
        score_a >= 0.0,
        "expected non-negative score for Peer A, got score {}",
        score_a
    );
    assert!(
        score_b >= 0.0,
        "expected non-negative score for Peer B, got score {}",
        score_b
    );

    // the penalty is the difference between the threshold and the actual mesh deliveries, squared.
    // since we didn't deliver anything, this is just the value of the threshold
    let penalty = topic_params.mesh_message_deliveries_threshold
        * topic_params.mesh_message_deliveries_threshold;
    let expected =
        topic_params.topic_weight * topic_params.mesh_message_deliveries_weight * penalty;

    assert!(
        score_c == expected,
        "Score: {}. Expected {}",
        score_c,
        expected
    );
}

#[test]
fn test_score_mesh_message_deliveries_decay() {
    // Create parameters with reasonable default values
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let mut params = PeerScoreParams::default();

    let mut topic_params = TopicScoreParams::default();
    topic_params.topic_weight = 1.0;
    topic_params.mesh_message_deliveries_weight = -1.0;
    topic_params.mesh_message_deliveries_activation = Duration::from_secs(0);
    topic_params.mesh_message_deliveries_window = Duration::from_millis(10);
    topic_params.mesh_message_deliveries_threshold = 20.0;
    topic_params.mesh_message_deliveries_cap = 100.0;
    topic_params.mesh_message_deliveries_decay = 0.9;
    topic_params.first_message_deliveries_weight = 0.0;
    topic_params.time_in_mesh_weight = 0.0;
    topic_params.time_in_mesh_quantum = Duration::from_secs(1);
    topic_params.mesh_failure_penalty_weight = 0.0;

    params.topics.insert(topic_hash, topic_params.clone());
    let mut peer_score = PeerScore::new(params);

    let peer_id_a = PeerId::random();
    peer_score.add_peer(peer_id_a.clone());
    peer_score.graft(&peer_id_a, topic.clone());

    // deliver a bunch of messages from peer A
    let messages = 100;
    for seq in 0..messages {
        let (id, msg) = make_test_message(seq);
        peer_score.validate_message(&peer_id_a, &id, &msg.topic);
        peer_score.deliver_message(&peer_id_a, &id, &msg.topic);
    }

    // we should have a positive score, since we delivered more messages than the threshold
    peer_score.refresh_scores();

    let score_a = peer_score.score(&peer_id_a);
    assert!(
        score_a >= 0.0,
        "expected non-negative score for Peer A, got score {}",
        score_a
    );

    let mut decayed_delivery_count = (messages as f64) * topic_params.mesh_message_deliveries_decay;
    for _ in 0..20 {
        peer_score.refresh_scores();
        decayed_delivery_count *= topic_params.mesh_message_deliveries_decay;
    }

    let score_a = peer_score.score(&peer_id_a);
    // the penalty is the difference between the threshold and the (decayed) mesh deliveries, squared.
    let deficit = topic_params.mesh_message_deliveries_threshold - decayed_delivery_count;
    let penalty = deficit * deficit;
    let expected =
        topic_params.topic_weight * topic_params.mesh_message_deliveries_weight * penalty;

    assert_eq!(score_a, expected, "Invalid score");
}

#[test]
fn test_score_mesh_failure_penalty() {
    // Create parameters with reasonable default values
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let mut params = PeerScoreParams::default();

    let mut topic_params = TopicScoreParams::default();
    // the mesh failure penalty is applied when a peer is pruned while their
    // mesh deliveries are under the threshold.
    // for this test, we set the mesh delivery threshold, but set
    // mesh_message_deliveries to zero, so the only affect on the score
    // is from the mesh failure penalty
    topic_params.topic_weight = 1.0;
    topic_params.mesh_message_deliveries_weight = 0.0;
    topic_params.mesh_message_deliveries_activation = Duration::from_secs(0);
    topic_params.mesh_message_deliveries_window = Duration::from_millis(10);
    topic_params.mesh_message_deliveries_threshold = 20.0;
    topic_params.mesh_message_deliveries_cap = 100.0;
    topic_params.mesh_message_deliveries_decay = 1.0;
    topic_params.first_message_deliveries_weight = 0.0;
    topic_params.time_in_mesh_weight = 0.0;
    topic_params.time_in_mesh_quantum = Duration::from_secs(1);
    topic_params.mesh_failure_penalty_weight = -1.0;
    topic_params.mesh_failure_penalty_decay = 1.0;

    params.topics.insert(topic_hash, topic_params.clone());
    let mut peer_score = PeerScore::new(params);

    let peer_id_a = PeerId::random();
    let peer_id_b = PeerId::random();

    let peers = vec![peer_id_a.clone(), peer_id_b.clone()];

    for peer_id in &peers {
        peer_score.add_peer(peer_id.clone());
        peer_score.graft(&peer_id, topic.clone());
    }

    // deliver a bunch of messages from peer A
    let messages = 100;
    for seq in 0..messages {
        let (id, msg) = make_test_message(seq);

        peer_score.validate_message(&peer_id_a, &id, &msg.topic);
        peer_score.deliver_message(&peer_id_a, &id, &msg.topic);
    }

    // peers A and B should both have zero scores, since the failure penalty hasn't been applied yet
    peer_score.refresh_scores();
    let score_a = peer_score.score(&peer_id_a);
    let score_b = peer_score.score(&peer_id_b);
    assert!(
        score_a >= 0.0,
        "expected non-negative score for Peer A, got score {}",
        score_a
    );
    assert!(
        score_b >= 0.0,
        "expected non-negative score for Peer B, got score {}",
        score_b
    );

    // prune peer B to apply the penalty
    peer_score.prune(&peer_id_b, topic.hash());
    peer_score.refresh_scores();
    let score_a = peer_score.score(&peer_id_a);

    assert_eq!(score_a, 0.0, "expected Peer A to have a 0");

    // penalty calculation is the same as for mesh_message_deliveries, but multiplied by
    // mesh_failure_penalty_weigh
    // instead of mesh_message_deliveries_weight
    let penalty = topic_params.mesh_message_deliveries_threshold
        * topic_params.mesh_message_deliveries_threshold;
    let expected = topic_params.topic_weight * topic_params.mesh_failure_penalty_weight * penalty;

    let score_b = peer_score.score(&peer_id_b);

    assert_eq!(score_b, expected, "Peer B should have expected score",);
}

#[test]
fn test_score_invalid_message_deliveries() {
    // Create parameters with reasonable default values
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let mut params = PeerScoreParams::default();

    let mut topic_params = TopicScoreParams::default();
    topic_params.topic_weight = 1.0;
    topic_params.mesh_message_deliveries_weight = 0.0;
    topic_params.mesh_message_deliveries_activation = Duration::from_secs(1);
    topic_params.mesh_message_deliveries_window = Duration::from_millis(10);
    topic_params.mesh_message_deliveries_threshold = 20.0;
    topic_params.mesh_message_deliveries_cap = 100.0;
    topic_params.mesh_message_deliveries_decay = 1.0;
    topic_params.first_message_deliveries_weight = 0.0;
    topic_params.time_in_mesh_weight = 0.0;
    topic_params.mesh_failure_penalty_weight = 0.0;

    topic_params.invalid_message_deliveries_weight = -1.0;
    topic_params.invalid_message_deliveries_decay = 1.0;

    params.topics.insert(topic_hash, topic_params.clone());
    let mut peer_score = PeerScore::new(params);

    let peer_id_a = PeerId::random();
    peer_score.add_peer(peer_id_a.clone());
    peer_score.graft(&peer_id_a, topic.clone());

    // reject a bunch of messages from peer A
    let messages = 100;
    for seq in 0..messages {
        let (id, msg) = make_test_message(seq);
        peer_score.reject_message(&peer_id_a, &id, &msg.topic, RejectReason::ValidationFailed);
    }

    peer_score.refresh_scores();
    let score_a = peer_score.score(&peer_id_a);

    let expected = topic_params.topic_weight
        * topic_params.invalid_message_deliveries_weight
        * (messages * messages) as f64;

    assert_eq!(score_a, expected, "Peer has unexpected score",);
}

#[test]
fn test_score_invalid_message_deliveris_decay() {
    // Create parameters with reasonable default values
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let mut params = PeerScoreParams::default();

    let mut topic_params = TopicScoreParams::default();
    topic_params.topic_weight = 1.0;
    topic_params.mesh_message_deliveries_weight = 0.0;
    topic_params.mesh_message_deliveries_activation = Duration::from_secs(1);
    topic_params.mesh_message_deliveries_window = Duration::from_millis(10);
    topic_params.mesh_message_deliveries_threshold = 20.0;
    topic_params.mesh_message_deliveries_cap = 100.0;
    topic_params.mesh_message_deliveries_decay = 1.0;
    topic_params.first_message_deliveries_weight = 0.0;
    topic_params.time_in_mesh_weight = 0.0;
    topic_params.mesh_failure_penalty_weight = 0.0;

    topic_params.invalid_message_deliveries_weight = -1.0;
    topic_params.invalid_message_deliveries_decay = 0.9;

    params.topics.insert(topic_hash, topic_params.clone());
    let mut peer_score = PeerScore::new(params);

    let peer_id_a = PeerId::random();
    peer_score.add_peer(peer_id_a.clone());
    peer_score.graft(&peer_id_a, topic.clone());

    // reject a bunch of messages from peer A
    let messages = 100;
    for seq in 0..messages {
        let (id, msg) = make_test_message(seq);
        peer_score.reject_message(&peer_id_a, &id, &msg.topic, RejectReason::ValidationFailed);
    }

    peer_score.refresh_scores();

    let decay = topic_params.invalid_message_deliveries_decay * messages as f64;

    let mut expected =
        topic_params.topic_weight * topic_params.invalid_message_deliveries_weight * decay * decay;

    let score_a = peer_score.score(&peer_id_a);
    assert_eq!(score_a, expected, "Peer has unexpected score");

    // refresh scores a few times to apply decay
    for _ in 0..10 {
        peer_score.refresh_scores();
        expected *= topic_params.invalid_message_deliveries_decay
            * topic_params.invalid_message_deliveries_decay;
    }

    let score_a = peer_score.score(&peer_id_a);
    assert_eq!(score_a, expected, "Peer has unexpected score");
}

#[test]
fn test_score_reject_message_deliveries() {
    // This tests adds coverage for the dark corners of rejection tracing

    // Create parameters with reasonable default values
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let mut params = PeerScoreParams::default();

    let mut topic_params = TopicScoreParams::default();
    topic_params.topic_weight = 1.0;
    topic_params.mesh_message_deliveries_weight = 0.0;
    topic_params.first_message_deliveries_weight = 0.0;
    topic_params.mesh_failure_penalty_weight = 0.0;
    topic_params.time_in_mesh_weight = 0.0;
    topic_params.time_in_mesh_quantum = Duration::from_secs(1);
    topic_params.invalid_message_deliveries_weight = -1.0;
    topic_params.invalid_message_deliveries_decay = 1.0;

    params.topics.insert(topic_hash, topic_params.clone());
    let mut peer_score = PeerScore::new(params);

    let peer_id_a = PeerId::random();
    let peer_id_b = PeerId::random();

    let peers = vec![peer_id_a.clone(), peer_id_b.clone()];

    for peer_id in &peers {
        peer_score.add_peer(peer_id.clone());
    }

    let (id, msg) = make_test_message(1);

    // these should have no effect in the score
    peer_score.reject_message(&peer_id_a, &id, &msg.topic, RejectReason::BlackListedPeer);
    peer_score.reject_message(&peer_id_a, &id, &msg.topic, RejectReason::BlackListedSource);
    peer_score.reject_message(&peer_id_a, &id, &msg.topic, RejectReason::ValidationIgnored);

    peer_score.refresh_scores();
    let score_a = peer_score.score(&peer_id_a);
    let score_b = peer_score.score(&peer_id_b);

    assert_eq!(score_a, 0.0, "Should have no effect on the score");
    assert_eq!(score_b, 0.0, "Should have no effect on the score");

    // insert a record in the message deliveries
    peer_score.validate_message(&peer_id_a, &id, &msg.topic);

    // this should have no effect in the score, and subsequent duplicate messages should have no
    // effect either
    peer_score.reject_message(&peer_id_a, &id, &msg.topic, RejectReason::ValidationIgnored);
    peer_score.duplicated_message(&peer_id_b, &id, &msg.topic);

    peer_score.refresh_scores();
    let score_a = peer_score.score(&peer_id_a);
    let score_b = peer_score.score(&peer_id_b);

    assert_eq!(score_a, 0.0, "Should have no effect on the score");
    assert_eq!(score_b, 0.0, "Should have no effect on the score");

    // now clear the delivery record
    peer_score.deliveries.clear();

    // insert a record in the message deliveries
    peer_score.validate_message(&peer_id_a, &id, &msg.topic);

    // this should have no effect in the score, and subsequent duplicate messages should have no
    // effect either
    peer_score.reject_message(&peer_id_a, &id, &msg.topic, RejectReason::ValidationIgnored);
    peer_score.duplicated_message(&peer_id_b, &id, &msg.topic);

    peer_score.refresh_scores();
    let score_a = peer_score.score(&peer_id_a);
    let score_b = peer_score.score(&peer_id_b);

    assert_eq!(score_a, 0.0, "Should have no effect on the score");
    assert_eq!(score_b, 0.0, "Should have no effect on the score");

    // now clear the delivery record
    peer_score.deliveries.clear();

    // insert a new record in the message deliveries
    peer_score.validate_message(&peer_id_a, &id, &msg.topic);

    // and reject the message to make sure duplicates are also penalized
    peer_score.reject_message(&peer_id_a, &id, &msg.topic, RejectReason::ValidationFailed);
    peer_score.duplicated_message(&peer_id_b, &id, &msg.topic);

    peer_score.refresh_scores();
    let score_a = peer_score.score(&peer_id_a);
    let score_b = peer_score.score(&peer_id_b);

    assert_eq!(score_a, -1.0, "Score should be effected");
    assert_eq!(score_b, -1.0, "Score should be effected");

    // now clear the delivery record again
    peer_score.deliveries.clear();

    // insert a new record in the message deliveries
    peer_score.validate_message(&peer_id_a, &id, &msg.topic);

    // and reject the message after a duplicate has arrived
    peer_score.duplicated_message(&peer_id_b, &id, &msg.topic);
    peer_score.reject_message(&peer_id_a, &id, &msg.topic, RejectReason::ValidationFailed);

    peer_score.refresh_scores();
    let score_a = peer_score.score(&peer_id_a);
    let score_b = peer_score.score(&peer_id_b);

    assert_eq!(score_a, -4.0, "Score should be effected");
    assert_eq!(score_b, -4.0, "Score should be effected");
}

#[test]
fn test_application_score() {
    // Create parameters with reasonable default values
    let app_specific_weight = 0.5;
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let mut params = PeerScoreParams::default();
    params.app_specific_weight = app_specific_weight;

    let mut topic_params = TopicScoreParams::default();
    topic_params.topic_weight = 1.0;
    topic_params.mesh_message_deliveries_weight = 0.0;
    topic_params.first_message_deliveries_weight = 0.0;
    topic_params.mesh_failure_penalty_weight = 0.0;
    topic_params.time_in_mesh_weight = 0.0;
    topic_params.time_in_mesh_quantum = Duration::from_secs(1);
    topic_params.invalid_message_deliveries_weight = 0.0;
    topic_params.invalid_message_deliveries_decay = 1.0;

    params.topics.insert(topic_hash, topic_params.clone());
    let mut peer_score = PeerScore::new(params);

    let peer_id_a = PeerId::random();
    peer_score.add_peer(peer_id_a.clone());
    peer_score.graft(&peer_id_a, topic.clone());

    let messages = 100;
    for i in -100..messages {
        let app_score_value = i as f64;
        peer_score.set_application_score(&peer_id_a, app_score_value);
        peer_score.refresh_scores();
        let score_a = peer_score.score(&peer_id_a);
        let expected = (i as f64) * app_specific_weight;
        assert_eq!(score_a, expected, "Peer has unexpected score");
    }
}

#[test]
fn test_score_ip_colocation() {
    // Create parameters with reasonable default values
    let ip_colocation_factor_weight = -1.0;
    let ip_colocation_factor_threshold = 1.0;
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let mut params = PeerScoreParams::default();
    params.ip_colocation_factor_weight = ip_colocation_factor_weight;
    params.ip_colocation_factor_threshold = ip_colocation_factor_threshold;

    let mut topic_params = TopicScoreParams::default();
    topic_params.topic_weight = 1.0;
    topic_params.mesh_message_deliveries_weight = 0.0;
    topic_params.first_message_deliveries_weight = 0.0;
    topic_params.mesh_failure_penalty_weight = 0.0;
    topic_params.time_in_mesh_weight = 0.0;
    topic_params.time_in_mesh_quantum = Duration::from_secs(1);
    topic_params.invalid_message_deliveries_weight = 0.0;

    params.topics.insert(topic_hash, topic_params.clone());
    let mut peer_score = PeerScore::new(params);

    let peer_id_a = PeerId::random();
    let peer_id_b = PeerId::random();
    let peer_id_c = PeerId::random();
    let peer_id_d = PeerId::random();

    let peers = vec![
        peer_id_a.clone(),
        peer_id_b.clone(),
        peer_id_c.clone(),
        peer_id_d.clone(),
    ];
    for peer_id in &peers {
        peer_score.add_peer(peer_id.clone());
        peer_score.graft(&peer_id, topic.clone());
    }

    // peerA should have no penalty, but B, C, and D should be penalized for sharing an IP
    peer_score.add_ip(&peer_id_a, "1.2.3.4".parse().unwrap());
    peer_score.add_ip(&peer_id_b, "2.3.4.5".parse().unwrap());
    peer_score.add_ip(&peer_id_c, "2.3.4.5".parse().unwrap());
    peer_score.add_ip(&peer_id_c, "3.4.5.6".parse().unwrap());
    peer_score.add_ip(&peer_id_d, "2.3.4.5".parse().unwrap());

    peer_score.refresh_scores();
    let score_a = peer_score.score(&peer_id_a);
    let score_b = peer_score.score(&peer_id_b);
    let score_c = peer_score.score(&peer_id_c);
    let score_d = peer_score.score(&peer_id_d);

    assert_eq!(score_a, 0.0, "Peer A should be unaffected");

    let n_shared = 3.0;
    let ip_surplus = n_shared - ip_colocation_factor_threshold;
    let penalty = ip_surplus * ip_surplus;
    let expected = ip_colocation_factor_weight * penalty as f64;

    assert_eq!(score_b, expected, "Peer B should have expected score");
    assert_eq!(score_c, expected, "Peer C should have expected score");
    assert_eq!(score_d, expected, "Peer D should have expected score");
}

#[test]
fn test_score_behaviour_penality() {
    // Create parameters with reasonable default values
    let behaviour_penalty_weight = -1.0;
    let behaviour_penalty_decay = 0.99;

    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let mut params = PeerScoreParams::default();
    params.behaviour_penalty_decay = behaviour_penalty_decay;
    params.behaviour_penalty_weight = behaviour_penalty_weight;

    let mut topic_params = TopicScoreParams::default();
    topic_params.topic_weight = 1.0;
    topic_params.mesh_message_deliveries_weight = 0.0;
    topic_params.first_message_deliveries_weight = 0.0;
    topic_params.mesh_failure_penalty_weight = 0.0;
    topic_params.time_in_mesh_weight = 0.0;
    topic_params.time_in_mesh_quantum = Duration::from_secs(1);
    topic_params.invalid_message_deliveries_weight = 0.0;

    params.topics.insert(topic_hash, topic_params.clone());
    let mut peer_score = PeerScore::new(params);

    let peer_id_a = PeerId::random();

    // add a penalty to a non-existent peer.
    peer_score.add_penalty(&peer_id_a, 1);

    let score_a = peer_score.score(&peer_id_a);
    assert_eq!(score_a, 0.0, "Peer A should be unaffected");

    // add the peer and test penalties
    peer_score.add_peer(peer_id_a.clone());
    assert_eq!(score_a, 0.0, "Peer A should be unaffected");

    peer_score.add_penalty(&peer_id_a, 1);

    let score_a = peer_score.score(&peer_id_a);
    assert_eq!(score_a, -1.0, "Peer A should have been penalized");

    peer_score.add_penalty(&peer_id_a, 1);
    let score_a = peer_score.score(&peer_id_a);
    assert_eq!(score_a, -4.0, "Peer A should have been penalized");

    peer_score.refresh_scores();

    let score_a = peer_score.score(&peer_id_a);
    assert_eq!(score_a, -3.9204, "Peer A should have been penalized");
}

#[test]
fn test_score_retention() {
    // Create parameters with reasonable default values
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let app_specific_weight = 1.0;
    let app_score_value = -1000.0;
    let retain_score = Duration::from_secs(1);
    let mut params = PeerScoreParams::default();
    params.app_specific_weight = app_specific_weight;
    params.retain_score = retain_score;

    let mut topic_params = TopicScoreParams::default();
    topic_params.topic_weight = 0.0;
    topic_params.mesh_message_deliveries_weight = 0.0;
    topic_params.mesh_message_deliveries_activation = Duration::from_secs(0);
    topic_params.first_message_deliveries_weight = 0.0;
    topic_params.time_in_mesh_weight = 0.0;

    params.topics.insert(topic_hash, topic_params.clone());
    let mut peer_score = PeerScore::new(params);

    let peer_id_a = PeerId::random();
    peer_score.add_peer(peer_id_a.clone());
    peer_score.graft(&peer_id_a, topic.clone());

    peer_score.set_application_score(&peer_id_a, app_score_value);

    // score should equal -1000 (app specific score)
    peer_score.refresh_scores();
    let score_a = peer_score.score(&peer_id_a);
    assert_eq!(
        score_a, app_score_value,
        "Score should be the application specific score"
    );

    // disconnect & wait half of RetainScore time. Should still have negative score
    peer_score.remove_peer(&peer_id_a);
    std::thread::sleep(retain_score / 2);
    peer_score.refresh_scores();
    let score_a = peer_score.score(&peer_id_a);
    assert_eq!(
        score_a, app_score_value,
        "Score should be the application specific score"
    );

    // wait remaining time (plus a little slop) and the score should reset to zero
    std::thread::sleep(retain_score / 2 + Duration::from_millis(50));
    peer_score.refresh_scores();
    let score_a = peer_score.score(&peer_id_a);
    assert_eq!(
        score_a, 0.0,
        "Score should be the application specific score"
    );
}
