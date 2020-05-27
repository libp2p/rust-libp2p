/// A collection of unit tests mostly ported from the go implementation.
use super::*;

use crate::IdentTopic as Topic;

// estimates a value within variance
fn within_variance(value: f64, expected: f64, variance: f64) -> bool {
    if expected >= 0.0 {
        return value > expected * (1.0 - variance) && value < expected * (1.0 + variance);
    }
    return value > expected * (1.0 + variance) && value < expected * (1.0 - variance);
}

// generates a random gossipsub message with sequence number i
fn make_test_message(seq: u64) -> GossipsubMessage {
    GossipsubMessage {
        source: PeerId::random(),
        data: vec![12, 34, 56],
        sequence_number: seq,
        topics: vec![Topic::new("test").hash()],
        signature: None,
        key: None,
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
    peer_score.add_peer(peer_id.clone(), Vec::new());

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
    peer_score.add_peer(peer_id.clone(), Vec::new());

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
    peer_score.add_peer(peer_id.clone(), Vec::new());
    peer_score.graft(&peer_id, topic);

    // deliver a bunch of messages from the peer
    let messages = 100;
    for seq in 0..messages {
        let msg = make_test_message(seq);
        peer_score.validate_message(&peer_id, &msg);
        peer_score.deliver_message(&peer_id, &msg);
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
    peer_score.add_peer(peer_id.clone(), Vec::new());
    peer_score.graft(&peer_id, topic);

    // deliver a bunch of messages from the peer
    let messages = 100;
    for seq in 0..messages {
        let msg = make_test_message(seq);
        peer_score.validate_message(&peer_id, &msg);
        peer_score.deliver_message(&peer_id, &msg);
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

/*
func TestScoreFirstMessageDeliveriesDecay(t *testing.T) {
    // Create parameters with reasonable default values
    mytopic := "mytopic"
    params := &PeerScoreParams{
        AppSpecificScore: func(peer.ID) float64 { return 0 },
        Topics:           make(map[string]*TopicScoreParams),
    }
    topicScoreParams := &TopicScoreParams{
        TopicWeight:                  1,
        FirstMessageDeliveriesWeight: 1,
        FirstMessageDeliveriesDecay:  0.9, // decay 10% per decay interval
        FirstMessageDeliveriesCap:    2000,
        TimeInMeshQuantum:            time.Second,
    }

    params.Topics[mytopic] = topicScoreParams
    peerA := peer.ID("A")

    ps := newPeerScore(params)
    ps.AddPeer(peerA, "myproto")
    ps.Graft(peerA, mytopic)

    // deliver a bunch of messages from peer A
    nMessages := 100
    for i := 0; i < nMessages; i++ {
        pbMsg := makeTestMessage(i)
        pbMsg.TopicIDs = []string{mytopic}
        msg := Message{ReceivedFrom: peerA, Message: pbMsg}
        ps.ValidateMessage(&msg)
        ps.DeliverMessage(&msg)
    }

    ps.refreshScores()
    aScore := ps.Score(peerA)
    expected := topicScoreParams.TopicWeight * topicScoreParams.FirstMessageDeliveriesWeight * topicScoreParams.FirstMessageDeliveriesDecay * float64(nMessages)
    if aScore != expected {
        t.Fatalf("Score: %f. Expected %f", aScore, expected)
    }

    // refreshing the scores applies the decay param
    decayIntervals := 10
    for i := 0; i < decayIntervals; i++ {
        ps.refreshScores()
        expected *= topicScoreParams.FirstMessageDeliveriesDecay
    }
    aScore = ps.Score(peerA)
    if aScore != expected {
        t.Fatalf("Score: %f. Expected %f", aScore, expected)
    }
}

func TestScoreMeshMessageDeliveries(t *testing.T) {
    // Create parameters with reasonable default values
    mytopic := "mytopic"
    params := &PeerScoreParams{
        AppSpecificScore: func(peer.ID) float64 { return 0 },
        Topics:           make(map[string]*TopicScoreParams),
    }
    topicScoreParams := &TopicScoreParams{
        TopicWeight:                     1,
        MeshMessageDeliveriesWeight:     -1,
        MeshMessageDeliveriesActivation: time.Second,
        MeshMessageDeliveriesWindow:     10 * time.Millisecond,
        MeshMessageDeliveriesThreshold:  20,
        MeshMessageDeliveriesCap:        100,
        MeshMessageDeliveriesDecay:      1.0, // no decay for this test

        FirstMessageDeliveriesWeight: 0,
        TimeInMeshQuantum:            time.Second,
    }

    params.Topics[mytopic] = topicScoreParams

    // peer A always delivers the message first.
    // peer B delivers next (within the delivery window).
    // peer C delivers outside the delivery window.
    // we expect peers A and B to have a score of zero, since all other parameter weights are zero.
    // Peer C should have a negative score.
    peerA := peer.ID("A")
    peerB := peer.ID("B")
    peerC := peer.ID("C")
    peers := []peer.ID{peerA, peerB, peerC}

    ps := newPeerScore(params)
    for _, p := range peers {
        ps.AddPeer(p, "myproto")
        ps.Graft(p, mytopic)
    }

    // assert that nobody has been penalized yet for not delivering messages before activation time
    ps.refreshScores()
    for _, p := range peers {
        score := ps.Score(p)
        if score < 0 {
            t.Fatalf("expected no mesh delivery penalty before activation time, got score %f", score)
        }
    }
    // wait for the activation time to kick in
    time.Sleep(topicScoreParams.MeshMessageDeliveriesActivation)

    // deliver a bunch of messages from peer A, with duplicates within the window from peer B,
    // and duplicates outside the window from peer C.
    nMessages := 100
    wg := sync.WaitGroup{}
    for i := 0; i < nMessages; i++ {
        pbMsg := makeTestMessage(i)
        pbMsg.TopicIDs = []string{mytopic}
        msg := Message{ReceivedFrom: peerA, Message: pbMsg}
        ps.ValidateMessage(&msg)
        ps.DeliverMessage(&msg)

        msg.ReceivedFrom = peerB
        ps.DuplicateMessage(&msg)

        // deliver duplicate from peerC after the window
        wg.Add(1)
        time.AfterFunc(topicScoreParams.MeshMessageDeliveriesWindow+(20*time.Millisecond), func() {
            msg.ReceivedFrom = peerC
            ps.DuplicateMessage(&msg)
            wg.Done()
        })
    }
    wg.Wait()

    ps.refreshScores()
    aScore := ps.Score(peerA)
    bScore := ps.Score(peerB)
    cScore := ps.Score(peerC)
    if aScore < 0 {
        t.Fatalf("Expected non-negative score for peer A, got %f", aScore)
    }
    if bScore < 0 {
        t.Fatalf("Expected non-negative score for peer B, got %f", aScore)
    }

    // the penalty is the difference between the threshold and the actual mesh deliveries, squared.
    // since we didn't deliver anything, this is just the value of the threshold
    penalty := topicScoreParams.MeshMessageDeliveriesThreshold * topicScoreParams.MeshMessageDeliveriesThreshold
    expected := topicScoreParams.TopicWeight * topicScoreParams.MeshMessageDeliveriesWeight * penalty
    if cScore != expected {
        t.Fatalf("Score: %f. Expected %f", cScore, expected)
    }
}

func TestScoreMeshMessageDeliveriesDecay(t *testing.T) {
    // Create parameters with reasonable default values
    mytopic := "mytopic"
    params := &PeerScoreParams{
        AppSpecificScore: func(peer.ID) float64 { return 0 },
        Topics:           make(map[string]*TopicScoreParams),
    }
    topicScoreParams := &TopicScoreParams{
        TopicWeight:                     1,
        MeshMessageDeliveriesWeight:     -1,
        MeshMessageDeliveriesActivation: 0,
        MeshMessageDeliveriesWindow:     10 * time.Millisecond,
        MeshMessageDeliveriesThreshold:  20,
        MeshMessageDeliveriesCap:        100,
        MeshMessageDeliveriesDecay:      0.9,

        FirstMessageDeliveriesWeight: 0,
        TimeInMeshQuantum:            time.Second,
    }

    params.Topics[mytopic] = topicScoreParams

    peerA := peer.ID("A")

    ps := newPeerScore(params)
    ps.AddPeer(peerA, "myproto")
    ps.Graft(peerA, mytopic)

    // deliver messages from peer A
    nMessages := 40
    for i := 0; i < nMessages; i++ {
        pbMsg := makeTestMessage(i)
        pbMsg.TopicIDs = []string{mytopic}
        msg := Message{ReceivedFrom: peerA, Message: pbMsg}
        ps.ValidateMessage(&msg)
        ps.DeliverMessage(&msg)
    }

    // we should have a positive score, since we delivered more messages than the threshold
    ps.refreshScores()
    aScore := ps.Score(peerA)
    if aScore < 0 {
        t.Fatalf("Expected non-negative score for peer A, got %f", aScore)
    }

    // we need to refresh enough times for the decay to bring us below the threshold
    decayedDeliveryCount := float64(nMessages) * topicScoreParams.MeshMessageDeliveriesDecay
    for i := 0; i < 20; i++ {
        ps.refreshScores()
        decayedDeliveryCount *= topicScoreParams.MeshMessageDeliveriesDecay
    }
    aScore = ps.Score(peerA)
    // the penalty is the difference between the threshold and the (decayed) mesh deliveries, squared.
    deficit := topicScoreParams.MeshMessageDeliveriesThreshold - decayedDeliveryCount
    penalty := deficit * deficit
    expected := topicScoreParams.TopicWeight * topicScoreParams.MeshMessageDeliveriesWeight * penalty
    if aScore != expected {
        t.Fatalf("Score: %f. Expected %f", aScore, expected)
    }
}

func TestScoreMeshFailurePenalty(t *testing.T) {
    // Create parameters with reasonable default values
    mytopic := "mytopic"
    params := &PeerScoreParams{
        AppSpecificScore: func(peer.ID) float64 { return 0 },
        Topics:           make(map[string]*TopicScoreParams),
    }

    // the mesh failure penalty is applied when a peer is pruned while their
    // mesh deliveries are under the threshold.
    // for this test, we set the mesh delivery threshold, but set
    // MeshMessageDeliveriesWeight to zero, so the only affect on the score
    // is from the mesh failure penalty
    topicScoreParams := &TopicScoreParams{
        TopicWeight:              1,
        MeshFailurePenaltyWeight: -1,
        MeshFailurePenaltyDecay:  1.0,

        MeshMessageDeliveriesActivation: 0,
        MeshMessageDeliveriesWindow:     10 * time.Millisecond,
        MeshMessageDeliveriesThreshold:  20,
        MeshMessageDeliveriesCap:        100,
        MeshMessageDeliveriesDecay:      1.0,

        MeshMessageDeliveriesWeight:  0,
        FirstMessageDeliveriesWeight: 0,
        TimeInMeshQuantum:            time.Second,
    }

    params.Topics[mytopic] = topicScoreParams

    peerA := peer.ID("A")
    peerB := peer.ID("B")
    peers := []peer.ID{peerA, peerB}

    ps := newPeerScore(params)
    for _, p := range peers {
        ps.AddPeer(p, "myproto")
        ps.Graft(p, mytopic)
    }

    // deliver messages from peer A. peer B does nothing
    nMessages := 100
    for i := 0; i < nMessages; i++ {
        pbMsg := makeTestMessage(i)
        pbMsg.TopicIDs = []string{mytopic}
        msg := Message{ReceivedFrom: peerA, Message: pbMsg}
        ps.ValidateMessage(&msg)
        ps.DeliverMessage(&msg)
    }

    // peers A and B should both have zero scores, since the failure penalty hasn't been applied yet
    ps.refreshScores()
    aScore := ps.Score(peerA)
    bScore := ps.Score(peerB)
    if aScore != 0 {
        t.Errorf("expected peer A to have score 0.0, got %f", aScore)
    }
    if bScore != 0 {
        t.Errorf("expected peer B to have score 0.0, got %f", bScore)
    }

    // prune peer B to apply the penalty
    ps.Prune(peerB, mytopic)
    ps.refreshScores()
    aScore = ps.Score(peerA)
    bScore = ps.Score(peerB)

    if aScore != 0 {
        t.Errorf("expected peer A to have score 0.0, got %f", aScore)
    }

    // penalty calculation is the same as for MeshMessageDeliveries, but multiplied by MeshFailurePenaltyWeight
    // instead of MeshMessageDeliveriesWeight
    penalty := topicScoreParams.MeshMessageDeliveriesThreshold * topicScoreParams.MeshMessageDeliveriesThreshold
    expected := topicScoreParams.TopicWeight * topicScoreParams.MeshFailurePenaltyWeight * penalty
    if bScore != expected {
        t.Fatalf("Score: %f. Expected %f", bScore, expected)
    }
}

func TestScoreInvalidMessageDeliveries(t *testing.T) {
    // Create parameters with reasonable default values
    mytopic := "mytopic"
    params := &PeerScoreParams{
        AppSpecificScore: func(peer.ID) float64 { return 0 },
        Topics:           make(map[string]*TopicScoreParams),
    }
    topicScoreParams := &TopicScoreParams{
        TopicWeight:                    1,
        TimeInMeshQuantum:              time.Second,
        InvalidMessageDeliveriesWeight: -1,
        InvalidMessageDeliveriesDecay:  1.0,
    }
    params.Topics[mytopic] = topicScoreParams

    peerA := peer.ID("A")

    ps := newPeerScore(params)
    ps.AddPeer(peerA, "myproto")
    ps.Graft(peerA, mytopic)

    nMessages := 100
    for i := 0; i < nMessages; i++ {
        pbMsg := makeTestMessage(i)
        pbMsg.TopicIDs = []string{mytopic}
        msg := Message{ReceivedFrom: peerA, Message: pbMsg}
        ps.RejectMessage(&msg, rejectInvalidSignature)
    }

    ps.refreshScores()
    aScore := ps.Score(peerA)
    expected := topicScoreParams.TopicWeight * topicScoreParams.InvalidMessageDeliveriesWeight * float64(nMessages*nMessages)
    if aScore != expected {
        t.Fatalf("Score: %f. Expected %f", aScore, expected)
    }
}

func TestScoreInvalidMessageDeliveriesDecay(t *testing.T) {
    // Create parameters with reasonable default values
    mytopic := "mytopic"
    params := &PeerScoreParams{
        AppSpecificScore: func(peer.ID) float64 { return 0 },
        Topics:           make(map[string]*TopicScoreParams),
    }
    topicScoreParams := &TopicScoreParams{
        TopicWeight:                    1,
        TimeInMeshQuantum:              time.Second,
        InvalidMessageDeliveriesWeight: -1,
        InvalidMessageDeliveriesDecay:  0.9,
    }
    params.Topics[mytopic] = topicScoreParams

    peerA := peer.ID("A")

    ps := newPeerScore(params)
    ps.AddPeer(peerA, "myproto")
    ps.Graft(peerA, mytopic)

    nMessages := 100
    for i := 0; i < nMessages; i++ {
        pbMsg := makeTestMessage(i)
        pbMsg.TopicIDs = []string{mytopic}
        msg := Message{ReceivedFrom: peerA, Message: pbMsg}
        ps.RejectMessage(&msg, rejectInvalidSignature)
    }

    ps.refreshScores()
    aScore := ps.Score(peerA)
    expected := topicScoreParams.TopicWeight * topicScoreParams.InvalidMessageDeliveriesWeight * math.Pow(topicScoreParams.InvalidMessageDeliveriesDecay*float64(nMessages), 2)
    if aScore != expected {
        t.Fatalf("Score: %f. Expected %f", aScore, expected)
    }

    // refresh scores a few times to apply decay
    for i := 0; i < 10; i++ {
        ps.refreshScores()
        expected *= math.Pow(topicScoreParams.InvalidMessageDeliveriesDecay, 2)
    }
    aScore = ps.Score(peerA)
    if aScore != expected {
        t.Fatalf("Score: %f. Expected %f", aScore, expected)
    }
}

func TestScoreRejectMessageDeliveries(t *testing.T) {
    // this tests adds coverage for the dark corners of rejection tracing
    mytopic := "mytopic"
    params := &PeerScoreParams{
        AppSpecificScore: func(peer.ID) float64 { return 0 },
        Topics:           make(map[string]*TopicScoreParams),
    }
    topicScoreParams := &TopicScoreParams{
        TopicWeight:                    1,
        TimeInMeshQuantum:              time.Second,
        InvalidMessageDeliveriesWeight: -1,
        InvalidMessageDeliveriesDecay:  1.0,
    }
    params.Topics[mytopic] = topicScoreParams

    peerA := peer.ID("A")
    peerB := peer.ID("B")

    ps := newPeerScore(params)
    ps.AddPeer(peerA, "myproto")
    ps.AddPeer(peerB, "myproto")

    pbMsg := makeTestMessage(0)
    pbMsg.TopicIDs = []string{mytopic}
    msg := Message{ReceivedFrom: peerA, Message: pbMsg}
    msg2 := Message{ReceivedFrom: peerB, Message: pbMsg}

    // these should have no effect in the score
    ps.RejectMessage(&msg, rejectBlacklstedPeer)
    ps.RejectMessage(&msg, rejectBlacklistedSource)
    ps.RejectMessage(&msg, rejectValidationQueueFull)

    aScore := ps.Score(peerA)
    expected := 0.0
    if aScore != expected {
        t.Fatalf("Score: %f. Expected %f", aScore, expected)
    }

    // insert a record in the message deliveries
    ps.ValidateMessage(&msg)

    // this should have no effect in the score, and subsequent duplicate messages should have no
    // effect either
    ps.RejectMessage(&msg, rejectValidationThrottled)
    ps.DuplicateMessage(&msg2)

    aScore = ps.Score(peerA)
    expected = 0.0
    if aScore != expected {
        t.Fatalf("Score: %f. Expected %f", aScore, expected)
    }

    bScore := ps.Score(peerB)
    expected = 0.0
    if bScore != expected {
        t.Fatalf("Score: %f. Expected %f", aScore, expected)
    }

    // now clear the delivery record
    ps.deliveries.head.expire = time.Now()
    time.Sleep(1 * time.Millisecond)
    ps.deliveries.gc()

    // insert a record in the message deliveries
    ps.ValidateMessage(&msg)

    // this should have no effect in the score, and subsequent duplicate messages should have no
    // effect either
    ps.RejectMessage(&msg, rejectValidationIgnored)
    ps.DuplicateMessage(&msg2)

    aScore = ps.Score(peerA)
    expected = 0.0
    if aScore != expected {
        t.Fatalf("Score: %f. Expected %f", aScore, expected)
    }

    bScore = ps.Score(peerB)
    expected = 0.0
    if bScore != expected {
        t.Fatalf("Score: %f. Expected %f", aScore, expected)
    }

    // now clear the delivery record
    ps.deliveries.head.expire = time.Now()
    time.Sleep(1 * time.Millisecond)
    ps.deliveries.gc()

    // insert a new record in the message deliveries
    ps.ValidateMessage(&msg)

    // and reject the message to make sure duplicates are also penalized
    ps.RejectMessage(&msg, rejectValidationFailed)
    ps.DuplicateMessage(&msg2)

    aScore = ps.Score(peerA)
    expected = -1.0
    if aScore != expected {
        t.Fatalf("Score: %f. Expected %f", aScore, expected)
    }

    bScore = ps.Score(peerB)
    expected = -1.0
    if bScore != expected {
        t.Fatalf("Score: %f. Expected %f", bScore, expected)
    }

    // now clear the delivery record again
    ps.deliveries.head.expire = time.Now()
    time.Sleep(1 * time.Millisecond)
    ps.deliveries.gc()

    // insert a new record in the message deliveries
    ps.ValidateMessage(&msg)

    // and reject the message after a duplciate has arrived
    ps.DuplicateMessage(&msg2)
    ps.RejectMessage(&msg, rejectValidationFailed)

    aScore = ps.Score(peerA)
    expected = -4.0
    if aScore != expected {
        t.Fatalf("Score: %f. Expected %f", aScore, expected)
    }

    bScore = ps.Score(peerB)
    expected = -4.0
    if bScore != expected {
        t.Fatalf("Score: %f. Expected %f", bScore, expected)
    }
}

func TestScoreApplicationScore(t *testing.T) {
    // Create parameters with reasonable default values
    mytopic := "mytopic"

    var appScoreValue float64
    params := &PeerScoreParams{
        AppSpecificScore:  func(peer.ID) float64 { return appScoreValue },
        AppSpecificWeight: 0.5,
        Topics:            make(map[string]*TopicScoreParams),
    }

    peerA := peer.ID("A")

    ps := newPeerScore(params)
    ps.AddPeer(peerA, "myproto")
    ps.Graft(peerA, mytopic)

    for i := -100; i < 100; i++ {
        appScoreValue = float64(i)
        ps.refreshScores()
        aScore := ps.Score(peerA)
        expected := float64(i) * params.AppSpecificWeight
        if aScore != expected {
            t.Errorf("expected peer score to equal app-specific score %f, got %f", expected, aScore)
        }
    }
}

func TestScoreIPColocation(t *testing.T) {
    // Create parameters with reasonable default values
    mytopic := "mytopic"

    params := &PeerScoreParams{
        AppSpecificScore:            func(peer.ID) float64 { return 0 },
        IPColocationFactorThreshold: 1,
        IPColocationFactorWeight:    -1,
        Topics:                      make(map[string]*TopicScoreParams),
    }

    peerA := peer.ID("A")
    peerB := peer.ID("B")
    peerC := peer.ID("C")
    peerD := peer.ID("D")
    peers := []peer.ID{peerA, peerB, peerC, peerD}

    ps := newPeerScore(params)
    for _, p := range peers {
        ps.AddPeer(p, "myproto")
        ps.Graft(p, mytopic)
    }

    // peerA should have no penalty, but B, C, and D should be penalized for sharing an IP
    setIPsForPeer(t, ps, peerA, "1.2.3.4")
    setIPsForPeer(t, ps, peerB, "2.3.4.5")
    setIPsForPeer(t, ps, peerC, "2.3.4.5", "3.4.5.6")
    setIPsForPeer(t, ps, peerD, "2.3.4.5")

    ps.refreshScores()
    aScore := ps.Score(peerA)
    bScore := ps.Score(peerB)
    cScore := ps.Score(peerC)
    dScore := ps.Score(peerD)

    if aScore != 0 {
        t.Errorf("expected peer A to have score 0.0, got %f", aScore)
    }

    nShared := 3
    ipSurplus := nShared - params.IPColocationFactorThreshold
    penalty := ipSurplus * ipSurplus
    expected := params.IPColocationFactorWeight * float64(penalty)
    for _, score := range []float64{bScore, cScore, dScore} {
        if score != expected {
            t.Fatalf("Score: %f. Expected %f", score, expected)
        }
    }
}

func TestScoreBehaviourPenalty(t *testing.T) {
    params := &PeerScoreParams{
        AppSpecificScore:       func(peer.ID) float64 { return 0 },
        BehaviourPenaltyWeight: -1,
        BehaviourPenaltyDecay:  0.99,
    }

    peerA := peer.ID("A")

    var ps *peerScore

    // first check AddPenalty on a nil peerScore
    ps.AddPenalty(peerA, 1)
    aScore := ps.Score(peerA)
    if aScore != 0 {
        t.Errorf("expected peer score to be 0, got %f", aScore)
    }

    // instantiate the peerScore
    ps = newPeerScore(params)

    // next AddPenalty on a non-existent peer
    ps.AddPenalty(peerA, 1)
    aScore = ps.Score(peerA)
    if aScore != 0 {
        t.Errorf("expected peer score to be 0, got %f", aScore)
    }

    // add the peer and test penalties
    ps.AddPeer(peerA, "myproto")

    aScore = ps.Score(peerA)
    if aScore != 0 {
        t.Errorf("expected peer score to be 0, got %f", aScore)
    }

    ps.AddPenalty(peerA, 1)
    aScore = ps.Score(peerA)
    if aScore != -1 {
        t.Errorf("expected peer score to be -1, got %f", aScore)
    }

    ps.AddPenalty(peerA, 1)
    aScore = ps.Score(peerA)
    if aScore != -4 {
        t.Errorf("expected peer score to be -4, got %f", aScore)
    }

    ps.refreshScores()

    aScore = ps.Score(peerA)
    if aScore != -3.9204 {
        t.Errorf("expected peer score to be -3.9204, got %f", aScore)
    }
}

func TestScoreRetention(t *testing.T) {
    // Create parameters with reasonable default values
    mytopic := "mytopic"

    params := &PeerScoreParams{
        AppSpecificScore:  func(peer.ID) float64 { return -1000 },
        AppSpecificWeight: 1.0,
        Topics:            make(map[string]*TopicScoreParams),
        RetainScore:       time.Second,
    }

    peerA := peer.ID("A")

    ps := newPeerScore(params)
    ps.AddPeer(peerA, "myproto")
    ps.Graft(peerA, mytopic)

    // score should equal -1000 (app specific score)
    expected := float64(-1000)
    ps.refreshScores()
    aScore := ps.Score(peerA)
    if aScore != expected {
        t.Fatalf("Score: %f. Expected %f", aScore, expected)
    }

    // disconnect & wait half of RetainScore time. should still have negative score
    ps.RemovePeer(peerA)
    delay := params.RetainScore / time.Duration(2)
    time.Sleep(delay)
    ps.refreshScores()
    aScore = ps.Score(peerA)
    if aScore != expected {
        t.Fatalf("Score: %f. Expected %f", aScore, expected)
    }

    // wait remaining time (plus a little slop) and the score should reset to zero
    time.Sleep(delay + (50 * time.Millisecond))
    ps.refreshScores()
    aScore = ps.Score(peerA)
    if aScore != 0 {
        t.Fatalf("Score: %f. Expected 0.0", aScore)
    }
}


// hack to set IPs for a peer without having to spin up real hosts with shared IPs
func setIPsForPeer(t *testing.T, ps *peerScore, p peer.ID, ips ...string) {
    t.Helper()
    ps.setIPs(p, ips, []string{})
    pstats, ok := ps.peerStats[p]
    if !ok {
        t.Fatal("unable to get peerStats")
    }
    pstats.ips = ips
}
*/
