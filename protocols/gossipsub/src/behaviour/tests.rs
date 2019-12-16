// collection of tests for the gossipsub network behaviour

#[cfg(test)]
mod tests {
    use super::super::*;

    // helper functions for testing

    // This function generates `peer_no` random PeerId's, subscribes to `topics` and subscribes the
    // injected nodes to all topics if `to_subscribe` is set. All nodes are considered gossipsub nodes.
    fn build_and_inject_nodes(
        peer_no: usize,
        topics: Vec<String>,
        to_subscribe: bool,
    ) -> (
        Gossipsub<tokio::net::TcpStream>,
        Vec<PeerId>,
        Vec<TopicHash>,
    ) {
        // generate a default GossipsubConfig
        let gs_config = GossipsubConfig::default();
        // create a gossipsub struct
        let mut gs: Gossipsub<tokio::net::TcpStream> = Gossipsub::new(PeerId::random(), gs_config);

        let mut topic_hashes = vec![];

        // subscribe to the topics
        for t in topics {
            let topic = Topic::new(t);
            gs.subscribe(topic.clone());
            topic_hashes.push(topic.no_hash().clone());
        }

        // build and connect peer_no random peers
        let mut peers = vec![];
        let dummy_connected_point = ConnectedPoint::Dialer {
            address: "/ip4/0.0.0.0/tcp/0".parse().unwrap(),
        };

        for _ in 0..peer_no {
            let peer = PeerId::random();
            peers.push(peer.clone());
            <Gossipsub<tokio::net::TcpStream> as NetworkBehaviour>::inject_connected(
                &mut gs,
                peer.clone(),
                dummy_connected_point.clone(),
            );
            if to_subscribe {
                gs.handle_received_subscriptions(
                    &topic_hashes
                        .iter()
                        .cloned()
                        .map(|t| GossipsubSubscription {
                            action: GossipsubSubscriptionAction::Subscribe,
                            topic_hash: t,
                        })
                        .collect::<Vec<_>>(),
                    &peer,
                );
            };
        }

        return (gs, peers, topic_hashes);
    }

    #[test]
    /// Test local node subscribing to a topic
    fn test_subscribe() {
        // The node should:
        // - Create an empty vector in mesh[topic]
        // - Send subscription request to all peers
        // - run JOIN(topic)

        let subscribe_topic = vec![String::from("test_subscribe")];
        let (gs, _, topic_hashes) = build_and_inject_nodes(20, subscribe_topic, true);

        assert!(
            gs.mesh.get(&topic_hashes[0]).is_some(),
            "Subscribe should add a new entry to the mesh[topic] hashmap"
        );

        // collect all the subscriptions
        let subscriptions =
            gs.events
                .iter()
                .fold(vec![], |mut collected_subscriptions, e| match e {
                    NetworkBehaviourAction::SendEvent { peer_id: _, event } => {
                        for s in &event.subscriptions {
                            match s.action {
                                GossipsubSubscriptionAction::Subscribe => {
                                    collected_subscriptions.push(s.clone())
                                }
                                _ => {}
                            };
                        }
                        collected_subscriptions
                    }
                    _ => collected_subscriptions,
                });

        // we sent a subscribe to all known peers
        assert!(
            subscriptions.len() == 20,
            "Should send a subscription to all known peers"
        );
    }

    #[test]
    /// Test unsubscribe.
    fn test_unsubscribe() {
        // Unsubscribe should:
        // - Remove the mesh entry for topic
        // - Send UNSUBSCRIBE to all known peers
        // - Call Leave

        let topic_strings = vec![String::from("topic1"), String::from("topic2")];
        let topics = topic_strings
            .iter()
            .map(|t| Topic::new(t.clone()))
            .collect::<Vec<Topic>>();

        // subscribe to topic_strings
        let (mut gs, _, topic_hashes) = build_and_inject_nodes(20, topic_strings, true);

        for topic_hash in &topic_hashes {
            assert!(
                gs.topic_peers.get(&topic_hash).is_some(),
                "Topic_peers contain a topic entry"
            );
            assert!(
                gs.mesh.get(&topic_hash).is_some(),
                "mesh should contain a topic entry"
            );
        }

        // unsubscribe from both topics
        assert!(
            gs.unsubscribe(topics[0].clone()),
            "should be able to unsubscribe successfully from each topic",
        );
        assert!(
            gs.unsubscribe(topics[1].clone()),
            "should be able to unsubscribe successfully from each topic",
        );

        let subscriptions =
            gs.events
                .iter()
                .fold(vec![], |mut collected_subscriptions, e| match e {
                    NetworkBehaviourAction::SendEvent { peer_id: _, event } => {
                        for s in &event.subscriptions {
                            match s.action {
                                GossipsubSubscriptionAction::Unsubscribe => {
                                    collected_subscriptions.push(s.clone())
                                }
                                _ => {}
                            };
                        }
                        collected_subscriptions
                    }
                    _ => collected_subscriptions,
                });

        // we sent a unsubscribe to all known peers, for two topics
        assert!(
            subscriptions.len() == 40,
            "Should send an unsubscribe event to all known peers"
        );

        // check we clean up internal structures
        for topic_hash in &topic_hashes {
            assert!(
                gs.mesh.get(&topic_hash).is_none(),
                "All topics should have been removed from the mesh"
            );
        }
    }

    #[test]
    /// Test JOIN(topic) functionality.
    fn test_join() {
        // The Join function should:
        // - Remove peers from fanout[topic]
        // - Add any fanout[topic] peers to the mesh (up to mesh_n)
        // - Fill up to mesh_n peers from known gossipsub peers in the topic
        // - Send GRAFT messages to all nodes added to the mesh

        // This test is not an isolated unit test, rather it uses higher level,
        // subscribe/unsubscribe to perform the test.

        let topic_strings = vec![String::from("topic1"), String::from("topic2")];
        let topics = topic_strings
            .iter()
            .map(|t| Topic::new(t.clone()))
            .collect::<Vec<Topic>>();

        let (mut gs, _, topic_hashes) = build_and_inject_nodes(20, topic_strings, true);

        // unsubscribe, then call join to invoke functionality
        assert!(
            gs.unsubscribe(topics[0].clone()),
            "should be able to unsubscribe successfully"
        );
        assert!(
            gs.unsubscribe(topics[1].clone()),
            "should be able to unsubscribe successfully"
        );

        // re-subscribe - there should be peers associated with the topic
        assert!(
            gs.subscribe(topics[0].clone()),
            "should be able to subscribe successfully"
        );

        // should have added mesh_n nodes to the mesh
        assert!(
            gs.mesh.get(&topic_hashes[0]).unwrap().len() == 6,
            "Should have added 6 nodes to the mesh"
        );

        // there should be mesh_n GRAFT messages.
        let graft_messages =
            gs.control_pool
                .iter()
                .fold(vec![], |mut collected_grafts, (_, controls)| {
                    for c in controls.iter() {
                        match c {
                            GossipsubControlAction::Graft { topic_hash: _ } => {
                                collected_grafts.push(c.clone())
                            }
                            _ => {}
                        }
                    }
                    collected_grafts
                });

        assert_eq!(
            graft_messages.len(),
            6,
            "There should be 6 grafts messages sent to peers"
        );

        // verify fanout nodes
        // add 3 random peers to the fanout[topic1]
        gs.fanout.insert(topic_hashes[1].clone(), vec![]);
        let new_peers = vec![];
        for _ in 0..3 {
            let fanout_peers = gs.fanout.get_mut(&topic_hashes[1]).unwrap();
            fanout_peers.push(PeerId::random());
        }

        // subscribe to topic1
        gs.subscribe(topics[1].clone());

        // the three new peers should have been added, along with 3 more from the pool.
        assert!(
            gs.mesh.get(&topic_hashes[1]).unwrap().len() == 6,
            "Should have added 6 nodes to the mesh"
        );
        let mesh_peers = gs.mesh.get(&topic_hashes[1]).unwrap();
        for new_peer in new_peers {
            assert!(
                mesh_peers.contains(new_peer),
                "Fanout peer should be included in the mesh"
            );
        }

        // there should now be 12 graft messages to be sent
        let graft_messages =
            gs.control_pool
                .iter()
                .fold(vec![], |mut collected_grafts, (_, controls)| {
                    for c in controls.iter() {
                        match c {
                            GossipsubControlAction::Graft { topic_hash: _ } => {
                                collected_grafts.push(c.clone())
                            }
                            _ => {}
                        }
                    }
                    collected_grafts
                });

        assert!(
            graft_messages.len() == 12,
            "There should be 12 grafts messages sent to peers"
        );
    }

    /// Test local node publish to subscribed topic
    #[test]
    fn test_publish() {
        // node should:
        // - Send publish message to all peers
        // - Insert message into gs.mcache and gs.received

        let publish_topic = String::from("test_publish");
        let (mut gs, _, topic_hashes) =
            build_and_inject_nodes(20, vec![publish_topic.clone()], true);

        assert!(
            gs.mesh.get(&topic_hashes[0]).is_some(),
            "Subscribe should add a new entry to the mesh[topic] hashmap"
        );

        // publish on topic
        let publish_data = vec![0; 42];
        gs.publish(&Topic::new(publish_topic), publish_data);

        // Collect all publish messages
        let publishes = gs
            .events
            .iter()
            .fold(vec![], |mut collected_publish, e| match e {
                NetworkBehaviourAction::SendEvent { peer_id: _, event } => {
                    for s in &event.messages {
                        collected_publish.push(s.clone());
                    }
                    collected_publish
                }
                _ => collected_publish,
            });

        let msg_id =
            (gs.config.message_id_fn)(&publishes.first().expect("Should contain > 0 entries"));

        assert!(
            publishes.len() == 20,
            "Should send a publish message to all known peers"
        );

        assert!(
            gs.mcache.get(&msg_id).is_some(),
            "Message cache should contain published message"
        );
        assert!(
            gs.received.get(&msg_id).is_some(),
            "Received cache should contain published message"
        );
    }

    /// Test local node publish to unsubscribed topic
    #[test]
    fn test_fanout() {
        // node should:
        // - Populate fanout peers
        // - Send publish message to fanout peers
        // - Insert message into gs.mcache and gs.received
        let fanout_topic = String::from("test_fanout");
        let (mut gs, _, topic_hashes) =
            build_and_inject_nodes(20, vec![fanout_topic.clone()], true);

        assert!(
            gs.mesh.get(&topic_hashes[0]).is_some(),
            "Subscribe should add a new entry to the mesh[topic] hashmap"
        );
        // Unsubscribe from topic
        assert!(
            gs.unsubscribe(Topic::new(fanout_topic.clone())),
            "should be able to unsubscribe successfully from topic"
        );

        // Publish on unsubscribed topic
        let publish_data = vec![0; 42];
        gs.publish(&Topic::new(fanout_topic.clone()), publish_data);

        assert_eq!(
            gs.fanout
                .get(&TopicHash::from_raw(fanout_topic.clone()))
                .unwrap()
                .len(),
            gs.config.mesh_n,
            "Fanout should contain `mesh_n` peers for fanout topic"
        );

        // Collect all publish messages
        let publishes = gs
            .events
            .iter()
            .fold(vec![], |mut collected_publish, e| match e {
                NetworkBehaviourAction::SendEvent { peer_id: _, event } => {
                    for s in &event.messages {
                        collected_publish.push(s.clone());
                    }
                    collected_publish
                }
                _ => collected_publish,
            });

        let msg_id =
            (gs.config.message_id_fn)(&publishes.first().expect("Should contain > 0 entries"));

        assert_eq!(
            publishes.len(),
            gs.config.mesh_n,
            "Should send a publish message to `mesh_n` fanout peers"
        );

        assert!(
            gs.mcache.get(&msg_id).is_some(),
            "Message cache should contain published message"
        );
        assert!(
            gs.received.get(&msg_id).is_some(),
            "Received cache should contain published message"
        );
    }

    #[test]
    /// Test the gossipsub NetworkBehaviour peer connection logic.
    fn test_inject_connected() {
        let (gs, peers, topic_hashes) = build_and_inject_nodes(
            20,
            vec![String::from("topic1"), String::from("topic2")],
            true,
        );

        // check that our subscriptions are sent to each of the peers
        // collect all the SendEvents
        let send_events: Vec<&NetworkBehaviourAction<Arc<GossipsubRpc>, GossipsubEvent>> = gs
            .events
            .iter()
            .filter(|e| match e {
                NetworkBehaviourAction::SendEvent {
                    peer_id: _,
                    event: _,
                } => true,
                _ => false,
            })
            .collect();

        // check that there are two subscriptions sent to each peer
        for sevent in send_events.clone() {
            match sevent {
                NetworkBehaviourAction::SendEvent { peer_id: _, event } => {
                    assert!(
                        event.subscriptions.len() == 2,
                        "There should be two subscriptions sent to each peer (1 for each topic)."
                    );
                }
                _ => {}
            };
        }

        // check that there are 20 send events created
        assert!(
            send_events.len() == 20,
            "There should be a subscription event sent to each peer."
        );

        // should add the new peers to `peer_topics` with an empty vec as a gossipsub node
        for peer in peers {
            let known_topics = gs.peer_topics.get(&peer).unwrap();
            assert!(
                known_topics == &topic_hashes,
                "The topics for each node should all topics"
            );
        }
    }

    #[test]
    /// Test subscription handling
    fn test_handle_received_subscriptions() {
        // For every subscription:
        // SUBSCRIBE:   - Add subscribed topic to peer_topics for peer.
        //              - Add peer to topics_peer.
        // UNSUBSCRIBE  - Remove topic from peer_topics for peer.
        //              - Remove peer from topic_peers.

        let topics = vec!["topic1", "topic2", "topic3", "topic4"]
            .iter()
            .map(|&t| String::from(t))
            .collect();
        let (mut gs, peers, topic_hashes) = build_and_inject_nodes(20, topics, false);

        // The first peer sends 3 subscriptions and 1 unsubscription
        let mut subscriptions = topic_hashes[..3]
            .iter()
            .map(|topic_hash| GossipsubSubscription {
                action: GossipsubSubscriptionAction::Subscribe,
                topic_hash: topic_hash.clone(),
            })
            .collect::<Vec<GossipsubSubscription>>();

        subscriptions.push(GossipsubSubscription {
            action: GossipsubSubscriptionAction::Unsubscribe,
            topic_hash: topic_hashes[topic_hashes.len() - 1].clone(),
        });

        let unknown_peer = PeerId::random();
        // process the subscriptions
        // first and second peers send subscriptions
        gs.handle_received_subscriptions(&subscriptions, &peers[0]);
        gs.handle_received_subscriptions(&subscriptions, &peers[1]);
        // unknown peer sends the same subscriptions
        gs.handle_received_subscriptions(&subscriptions, &unknown_peer);

        // verify the result

        let peer_topics = gs.peer_topics.get(&peers[0]).unwrap().clone();
        assert!(
            peer_topics == topic_hashes[..3].to_vec(),
            "First peer should be subscribed to three topics"
        );
        let peer_topics = gs.peer_topics.get(&peers[1]).unwrap().clone();
        assert!(
            peer_topics == topic_hashes[..3].to_vec(),
            "Second peer should be subscribed to three topics"
        );

        assert!(
            gs.peer_topics.get(&unknown_peer).is_none(),
            "Unknown peer should not have been added"
        );

        for topic_hash in topic_hashes[..3].iter() {
            let topic_peers = gs.topic_peers.get(topic_hash).unwrap().clone();
            assert!(
                topic_peers == peers[..2].to_vec(),
                "Two peers should be added to the first three topics"
            );
        }

        // Peer 0 unsubscribes from the first topic

        gs.handle_received_subscriptions(
            &vec![GossipsubSubscription {
                action: GossipsubSubscriptionAction::Unsubscribe,
                topic_hash: topic_hashes[0].clone(),
            }],
            &peers[0],
        );

        let peer_topics = gs.peer_topics.get(&peers[0]).unwrap().clone();
        assert!(
            peer_topics == topic_hashes[1..3].to_vec(),
            "Peer should be subscribed to two topics"
        );

        let topic_peers = gs.topic_peers.get(&topic_hashes[0]).unwrap().clone(); // only gossipsub at the moment
        assert!(
            topic_peers == peers[1..2].to_vec(),
            "Only the second peers should be in the first topic"
        );
    }

    #[test]
    /// Test Gossipsub.get_random_peers() function
    fn test_get_random_peers() {
        // generate a default GossipsubConfig
        let gs_config = GossipsubConfig::default();
        // create a gossipsub struct
        let mut gs: Gossipsub<usize> = Gossipsub::new(PeerId::random(), gs_config);

        // create a topic and fill it with some peers
        let topic_hash = Topic::new("Test".into()).no_hash().clone();
        let mut peers = vec![];
        for _ in 0..20 {
            peers.push(PeerId::random())
        }

        gs.topic_peers.insert(topic_hash.clone(), peers.clone());

        let random_peers =
            Gossipsub::<usize>::get_random_peers(&gs.topic_peers, &topic_hash, 5, { |_| true });
        assert!(random_peers.len() == 5, "Expected 5 peers to be returned");
        let random_peers =
            Gossipsub::<usize>::get_random_peers(&gs.topic_peers, &topic_hash, 30, { |_| true });
        assert!(random_peers.len() == 20, "Expected 20 peers to be returned");
        assert!(random_peers == peers, "Expected no shuffling");
        let random_peers =
            Gossipsub::<usize>::get_random_peers(&gs.topic_peers, &topic_hash, 20, { |_| true });
        assert!(random_peers.len() == 20, "Expected 20 peers to be returned");
        assert!(random_peers == peers, "Expected no shuffling");
        let random_peers =
            Gossipsub::<usize>::get_random_peers(&gs.topic_peers, &topic_hash, 0, { |_| true });
        assert!(random_peers.len() == 0, "Expected 0 peers to be returned");
        // test the filter
        let random_peers =
            Gossipsub::<usize>::get_random_peers(&gs.topic_peers, &topic_hash, 5, { |_| false });
        assert!(random_peers.len() == 0, "Expected 0 peers to be returned");
        let random_peers =
            Gossipsub::<usize>::get_random_peers(&gs.topic_peers, &topic_hash, 10, {
                |peer| peers.contains(peer)
            });
        assert!(random_peers.len() == 10, "Expected 10 peers to be returned");
    }

    /// Tests that the correct message is sent when a peer asks for a message in our cache.
    #[test]
    fn test_handle_iwant_msg_cached() {
        let (mut gs, peers, _) = build_and_inject_nodes(20, Vec::new(), true);

        let id = gs.config.message_id_fn;

        let message = GossipsubMessage {
            source: peers[11].clone(),
            data: vec![1, 2, 3, 4],
            sequence_number: 1u64,
            topics: Vec::new(),
        };
        let msg_id = id(&message);
        gs.mcache.put(message.clone());

        gs.handle_iwant(&peers[7], vec![msg_id.clone()]);

        // the messages we are sending
        let sent_messages = gs
            .events
            .iter()
            .fold(vec![], |mut collected_messages, e| match e {
                NetworkBehaviourAction::SendEvent { peer_id: _, event } => {
                    for c in &event.messages {
                        collected_messages.push(c.clone())
                    }
                    collected_messages
                }
                _ => collected_messages,
            });

        assert!(
            sent_messages.iter().any(|msg| id(msg) == msg_id),
            "Expected the cached message to be sent to an IWANT peer"
        );
    }

    /// Tests that messages are sent correctly depending on the shifting of the message cache.
    #[test]
    fn test_handle_iwant_msg_cached_shifted() {
        let (mut gs, peers, _) = build_and_inject_nodes(20, Vec::new(), true);

        let id = gs.config.message_id_fn;
        // perform 10 memshifts and check that it leaves the cache
        for shift in 1..10 {
            let message = GossipsubMessage {
                source: peers[11].clone(),
                data: vec![1, 2, 3, 4],
                sequence_number: shift,
                topics: Vec::new(),
            };
            let msg_id = id(&message);
            gs.mcache.put(message.clone());
            for _ in 0..shift {
                gs.mcache.shift();
            }

            gs.handle_iwant(&peers[7], vec![msg_id.clone()]);

            // is the message is being sent?
            let message_exists = gs.events.iter().any(|e| match e {
                NetworkBehaviourAction::SendEvent { peer_id: _, event } => {
                    event.messages.iter().any(|msg| id(msg) == msg_id)
                }
                _ => false,
            });
            // default history_length is 5, expect no messages after shift > 5
            if shift < 5 {
                assert!(
                    message_exists,
                    "Expected the cached message to be sent to an IWANT peer before 5 shifts"
                );
            } else {
                assert!(
                    !message_exists,
                    "Expected the cached message to not be sent to an IWANT peer after 5 shifts"
                );
            }
        }
    }

    #[test]
    // tests that an event is not created when a peers asks for a message not in our cache
    fn test_handle_iwant_msg_not_cached() {
        let (mut gs, peers, _) = build_and_inject_nodes(20, Vec::new(), true);

        let events_before = gs.events.len();
        gs.handle_iwant(&peers[7], vec![MessageId(String::from("unknown id"))]);
        let events_after = gs.events.len();

        assert_eq!(
            events_before, events_after,
            "Expected event count to stay the same"
        );
    }

    #[test]
    // tests that an event is created when a peer shares that it has a message we want
    fn test_handle_ihave_subscribed_and_msg_not_cached() {
        let (mut gs, peers, topic_hashes) =
            build_and_inject_nodes(20, vec![String::from("topic1")], true);

        gs.handle_ihave(
            &peers[7],
            vec![(
                topic_hashes[0].clone(),
                vec![MessageId(String::from("unknown id"))],
            )],
        );

        // check that we sent an IWANT request for `unknown id`
        let iwant_exists = match gs.control_pool.get(&peers[7]) {
            Some(controls) => controls.iter().any(|c| match c {
                GossipsubControlAction::IWant { message_ids } => message_ids
                    .iter()
                    .any(|m| *m.0 == String::from("unknown id")),
                _ => false,
            }),
            _ => false,
        };

        assert!(
            iwant_exists,
            "Expected to send an IWANT control message for unkown message id"
        );
    }

    #[test]
    // tests that an event is not created when a peer shares that it has a message that
    // we already have
    fn test_handle_ihave_subscribed_and_msg_cached() {
        let (mut gs, peers, topic_hashes) =
            build_and_inject_nodes(20, vec![String::from("topic1")], true);

        let msg_id = MessageId(String::from("known id"));
        gs.received.put(msg_id.clone(), ());

        let events_before = gs.events.len();
        gs.handle_ihave(&peers[7], vec![(topic_hashes[0].clone(), vec![msg_id])]);
        let events_after = gs.events.len();

        assert_eq!(
            events_before, events_after,
            "Expected event count to stay the same"
        )
    }

    #[test]
    // test that an event is not created when a peer shares that it has a message in
    // a topic that we are not subscribed to
    fn test_handle_ihave_not_subscribed() {
        let (mut gs, peers, _) = build_and_inject_nodes(20, vec![], true);

        let events_before = gs.events.len();
        gs.handle_ihave(
            &peers[7],
            vec![(
                TopicHash::from_raw(String::from("unsubscribed topic")),
                vec![MessageId(String::from("irrelevant id"))],
            )],
        );
        let events_after = gs.events.len();

        assert_eq!(
            events_before, events_after,
            "Expected event count to stay the same"
        )
    }

    #[test]
    // tests that a peer is added to our mesh when we are both subscribed
    // to the same topic
    fn test_handle_graft_is_subscribed() {
        let (mut gs, peers, topic_hashes) =
            build_and_inject_nodes(20, vec![String::from("topic1")], true);

        gs.handle_graft(&peers[7], topic_hashes.clone());

        assert!(
            gs.mesh.get(&topic_hashes[0]).unwrap().contains(&peers[7]),
            "Expected peer to have been added to mesh"
        );
    }

    #[test]
    // tests that a peer is not added to our mesh when they are subscribed to
    // a topic that we are not
    fn test_handle_graft_is_not_subscribed() {
        let (mut gs, peers, topic_hashes) =
            build_and_inject_nodes(20, vec![String::from("topic1")], true);

        gs.handle_graft(
            &peers[7],
            vec![TopicHash::from_raw(String::from("unsubscribed topic"))],
        );

        assert!(
            gs.mesh.get(&topic_hashes[0]).unwrap().contains(&peers[7]),
            "Expected peer to have been added to mesh"
        );
    }

    #[test]
    // tests multiple topics in a single graft message
    fn test_handle_graft_multiple_topics() {
        let topics: Vec<String> = vec!["topic1", "topic2", "topic3", "topic4"]
            .iter()
            .map(|&t| String::from(t))
            .collect();

        let (mut gs, peers, topic_hashes) = build_and_inject_nodes(20, topics.clone(), true);

        let mut their_topics = topic_hashes.clone();
        // their_topics = [topic1, topic2, topic3]
        // our_topics = [topic1, topic2, topic4]
        their_topics.pop();
        gs.leave(&their_topics[2]);

        gs.handle_graft(&peers[7], their_topics.clone());

        for i in 0..2 {
            assert!(
                gs.mesh.get(&topic_hashes[i]).unwrap().contains(&peers[7]),
                "Expected peer to be in the mesh for the first 2 topics"
            );
        }

        assert!(
            gs.mesh.get(&topic_hashes[2]).is_none(),
            "Expected the second topic to not be in the mesh"
        );
    }

    #[test]
    // tests that a peer is removed from our mesh
    fn test_handle_prune_peer_in_mesh() {
        let (mut gs, peers, topic_hashes) =
            build_and_inject_nodes(20, vec![String::from("topic1")], true);

        // insert peer into our mesh for 'topic1'
        gs.mesh.insert(topic_hashes[0].clone(), peers.clone());
        assert!(
            gs.mesh.get(&topic_hashes[0]).unwrap().contains(&peers[7]),
            "Expected peer to be in mesh"
        );

        gs.handle_prune(&peers[7], topic_hashes.clone());
        assert!(
            !gs.mesh.get(&topic_hashes[0]).unwrap().contains(&peers[7]),
            "Expected peer to be removed from mesh"
        );
    }
}
