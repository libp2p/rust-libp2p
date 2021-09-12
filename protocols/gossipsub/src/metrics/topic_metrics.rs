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
use log::{debug, error, warn};
use std::collections::{BTreeSet, HashMap};
use std::ops::AddAssign;

use libp2p_core::PeerId;
use strum::IntoEnumIterator;
use strum_macros::{EnumIter, IntoStaticStr};

#[derive(Default, Clone)]
/// This struct stores all the metrics for a given mesh slot.
/// NOTE: all the `message_*` counters refer to messages received from peers assigned to
/// this mesh slot on the topic this slot is associated with. See [`TopicMetrics`] for more
/// information.
pub struct SlotMetricCounts {
    /// The number of times this slot has been assigned to a peer
    assign_sum: u32,
    /// The total number of messages received
    messages_all: u32,
    /// The number of duplicate (already seen) messages
    /// A large number across peers on this topic could indicate a large amplification on
    /// the topic. Lowering the gossip_D parameter could help minimize duplicates.
    messages_duplicates: u32,
    /// The number of never before seen messages
    messages_first: u32,
    /// The number of messages that returned [`MessageAcceptance::Ignore`] from validation
    messages_ignored: u32,
    /// The number of messages that returned [`MessageAcceptance::Reject`] from validation
    messages_rejected: u32,
    /// The number of messages that returned [`MessageAcceptance::Accept`] from validation
    messages_validated: u32,
    /// The number of times this slot was churned due to us disconnecting from the peer
    churn_disconnected: u32,
    /// The number of times this slot was churned due to us removing excess mesh peers
    churn_excess: u32,
    /// The number of times this slot was churned due to us leaving the topic
    churn_leave: u32,
    /// The number of times this slot was churned because the peer sent us a PRUNE message
    churn_prune: u32,
    /// The number of times this slot was churned because the peer score was too low
    /// A large number could indicate the network is being attacked or the peer scoring
    /// parameters are too restrictive and need to be adjusted.
    churn_score: u32,
    /// The total number of times this slot was churned for any reason
    churn_sum: u32,
    /// The number of times this slot was churned due to the peer unsubscribing from the topic
    churn_unsubscribed: u32,
    /// The peer currently assigned to this slot (None if slot is vacant)
    current_peer: Option<Box<PeerId>>,
}

#[derive(IntoStaticStr, EnumIter, Clone, Copy)]
/// This enumerates all the message metric counters for a slot
pub enum SlotMessageMetric {
    /// Total messages received
    MessagesAll,
    /// Number of duplicate (already seen) messages
    MessagesDuplicates,
    /// Never before seen messages
    MessagesFirst,
    /// Messages that returned [`MessageAcceptance::Ignore`] from validation
    MessagesIgnored,
    /// Messages that returned [`MessageAcceptance::Reject`] from validation
    MessagesRejected,
    /// Messages that returned [`MessageAcceptance::Accept`] from validation
    MessagesValidated,
}

#[derive(IntoStaticStr, EnumIter, Clone, Copy)]
/// This enumerates all the churn metric counters for a slot
pub enum SlotChurnMetric {
    /// Slot churned due to us disconnecting from the peer
    ChurnDisconnected,
    /// Slot churned due to us removing excess mesh peers
    ChurnExcess,
    /// Slot churned due to us leaving the topic
    ChurnLeave,
    /// Slot churned because the peer sent us a PRUNE message
    ChurnPrune,
    /// Slot churned because the peer score was too low
    ChurnScore,
    /// Slot churned due to the peer unsubscribing from the topic
    ChurnUnsubscribed,
}

/// This enumerates ALL metric counters for a slot
pub enum SlotMetric {
    MessageMetric(SlotMessageMetric),
    ChurnMetric(SlotChurnMetric),
    /// Total times the slot was churned for any reason
    ChurnSum,
    /// Total times the slot was assigned
    AssignSum,
}

impl SlotMetric {
    /// Make SlotMetric iterable over every metric type
    pub fn iter() -> impl Iterator<Item = SlotMetric> {
        SlotMessageMetric::iter()
            .map(|message_metric| SlotMetric::MessageMetric(message_metric))
            .chain(
                SlotChurnMetric::iter().map(|churn_metric| SlotMetric::ChurnMetric(churn_metric)),
            )
            .chain(std::iter::once(SlotMetric::ChurnSum))
            .chain(std::iter::once(SlotMetric::AssignSum))
    }
}

impl SlotMetricCounts {
    pub fn new() -> Self {
        SlotMetricCounts::default()
    }

    /// returns a message metric count
    fn get_message_metric(&self, message_metric: SlotMessageMetric) -> u32 {
        match message_metric {
            SlotMessageMetric::MessagesAll => self.messages_all,
            SlotMessageMetric::MessagesDuplicates => self.messages_duplicates,
            SlotMessageMetric::MessagesFirst => self.messages_first,
            SlotMessageMetric::MessagesIgnored => self.messages_ignored,
            SlotMessageMetric::MessagesRejected => self.messages_rejected,
            SlotMessageMetric::MessagesValidated => self.messages_validated,
        }
    }

    /// increments a message metric count
    fn increment_message_metric(&mut self, message_metric: SlotMessageMetric) {
        match message_metric {
            SlotMessageMetric::MessagesAll => self.messages_all.add_assign(1),
            SlotMessageMetric::MessagesDuplicates => self.messages_duplicates.add_assign(1),
            SlotMessageMetric::MessagesFirst => self.messages_first.add_assign(1),
            SlotMessageMetric::MessagesIgnored => self.messages_ignored.add_assign(1),
            SlotMessageMetric::MessagesRejected => self.messages_rejected.add_assign(1),
            SlotMessageMetric::MessagesValidated => self.messages_validated.add_assign(1),
        };
    }

    /// returns a churn metric count
    fn get_churn_metric(&self, churn_reason: SlotChurnMetric) -> u32 {
        match churn_reason {
            SlotChurnMetric::ChurnDisconnected => self.churn_disconnected,
            SlotChurnMetric::ChurnExcess => self.churn_excess,
            SlotChurnMetric::ChurnLeave => self.churn_leave,
            SlotChurnMetric::ChurnPrune => self.churn_prune,
            SlotChurnMetric::ChurnScore => self.churn_score,
            SlotChurnMetric::ChurnUnsubscribed => self.churn_unsubscribed,
        }
    }

    /// churns a slot, incrementing the proper churn metric and returning ChurnSum
    pub fn churn_slot(&mut self, churn_reason: SlotChurnMetric) -> u32 {
        self.current_peer = None;
        self.churn_sum.add_assign(1);
        match churn_reason {
            SlotChurnMetric::ChurnDisconnected => self.churn_disconnected.add_assign(1),
            SlotChurnMetric::ChurnExcess => self.churn_excess.add_assign(1),
            SlotChurnMetric::ChurnLeave => self.churn_leave.add_assign(1),
            SlotChurnMetric::ChurnPrune => self.churn_prune.add_assign(1),
            SlotChurnMetric::ChurnScore => self.churn_score.add_assign(1),
            SlotChurnMetric::ChurnUnsubscribed => self.churn_unsubscribed.add_assign(1),
        };
        self.churn_sum
    }

    /// returns the current peer associated with this slot
    pub fn current_peer(&self) -> &Option<Box<PeerId>> {
        &self.current_peer
    }

    /// assigns a peer to this slot, incrementing and returning AssignSum
    pub fn assign_slot(&mut self, peer: PeerId) -> u32 {
        self.current_peer = Some(Box::new(peer));
        self.assign_sum.add_assign(1);
        self.assign_sum
    }

    /// returns the slot metric count corresponding to slot_metric
    pub fn get_slot_metric(&self, slot_metric: SlotMetric) -> u32 {
        match slot_metric {
            SlotMetric::MessageMetric(message_metric) => self.get_message_metric(message_metric),
            SlotMetric::ChurnMetric(churn_reason) => self.get_churn_metric(churn_reason),
            SlotMetric::ChurnSum => self.churn_sum,
            SlotMetric::AssignSum => self.assign_sum,
        }
    }

    /// returns a vector of pairs of all slot metric names and their corresponding counts
    pub fn with_names(&self) -> Vec<(&'static str, u32)> {
        SlotMetric::iter()
            .map(|t| match t {
                SlotMetric::MessageMetric(message_metric) => (
                    <SlotMessageMetric as Into<&'static str>>::into(message_metric),
                    self.get_message_metric(message_metric),
                ),
                SlotMetric::ChurnMetric(churn_reason) => (
                    <SlotChurnMetric as Into<&'static str>>::into(churn_reason),
                    self.get_churn_metric(churn_reason),
                ),
                SlotMetric::ChurnSum => ("ChurnSum", self.churn_sum),
                SlotMetric::AssignSum => ("AssignSum", self.assign_sum),
            })
            .collect()
    }
}

pub type MeshSlot = usize;
/// This structure stores all metrics associated with a single topic.
/// This introduces the concept of a mesh slot. When a peer is added to the mesh for
/// this topic, it is assigned to a mesh slot. All the metrics relating to messages
/// received from that peer on this topic are then associated to that slot. See the
/// [`SlotMetricCounts`] struct for more information. When a peer exits the mesh,
/// the slot it occupies is 'churned' and becomes vacant. Vacant slots are later
/// re-assigned when a new peer enters the mesh for this topic.
pub struct TopicMetrics {
    /// The topic this is associated with (useful for debugging)
    topic: TopicHash,
    /// Vector of SlotMetricCounts (indexed by MeshSlot)
    slot_metrics: Vec<SlotMetricCounts>,
    /// Map of PeerId to MeshSlot
    peer_slots: HashMap<PeerId, MeshSlot>,
    /// Set of Vacant MeshSlots (due to peers leaving the mesh)
    vacant_slots: BTreeSet<MeshSlot>,
    /// The number of messages requested via IWANT
    /// A large value indicates the mesh isn't performing as optimally as we would
    /// like and we have had to request extra messages via gossip
    pub iwant_requests: u32,
}

impl TopicMetrics {
    pub fn new(topic: TopicHash) -> Self {
        TopicMetrics {
            topic,
            // the first element in the vector is for peers that aren't in the mesh
            slot_metrics: vec![SlotMetricCounts::new()],
            peer_slots: HashMap::new(),
            vacant_slots: BTreeSet::new(),
            iwant_requests: 0,
        }
    }

    /// Increments the message metric for the specified peer
    pub fn increment_message_metric(&mut self, peer: &PeerId, message_metric: SlotMessageMetric) {
        let slot = self
            .peer_slots
            .get(peer)
            .map(|s| *s)
            // peers that aren't in the mesh get slot 0
            .unwrap_or(0);
        match self
            .slot_metrics
            .get_mut(slot)
        {
            Some(metric_counts) => metric_counts.increment_message_metric(message_metric),
            None => error!(
                "metrics_event[{}]: [slot {:02}] increment {} peer {} FAILURE [peer_slots contains peer with slot not existing in slot_metrics!]",
                self.topic,
                slot,
                <SlotMessageMetric as Into<&'static str>>::into(message_metric),
                peer,
            ),
        };
    }

    /// Assigns a slot to the peer if the peer doesn't already have one. Note that the
    /// lowest vacant slots are assigned first. If all slots are occupied, a new slot
    /// will be allocated.
    pub fn assign_slot_if_unassigned(&mut self, peer: PeerId) {
        if let std::collections::hash_map::Entry::Vacant(entry) = self.peer_slots.entry(peer) {
            match self.vacant_slots.iter().next() {
                Some(slot_ref) => match self.slot_metrics.get_mut(*slot_ref) {
                    // vacant slot available, assign new peer to this slot
                    Some(metric_counts) => {
                        let slot = *slot_ref;
                        let assign_sum = metric_counts.assign_slot(peer);
                        self.vacant_slots.remove(&slot);
                        entry.insert(slot);
                        debug!(
                            "metrics_event[{}]: [slot {:02}] assigning vacant slot to peer {} SUCCESS AssignSum[{}]",
                                self.topic, slot, peer, assign_sum,
                        );
                    },
                    None => error!(
                        "metrics_event[{}]: [slot {:02}] assigning vacant slot to peer {} FAILURE [SlotMetricCounts doesn't exist in slot_metrics vector!]",
                            self.topic, slot_ref, peer
                    ),
                },
                None => {
                    // No vacant slots available, allocate a new slot
                    let slot = self.slot_metrics.len();
                    let mut metric_counts = SlotMetricCounts::new();
                    let assign_sum = metric_counts.assign_slot(peer);
                    self.slot_metrics.push(metric_counts);
                    entry.insert(slot);
                    debug!(
                        "metrics_event[{}]: [slot {:02}] assigning new slot to peer {} SUCCESS AssignSum[{}]",
                            self.topic, slot, peer, assign_sum,
                    );
                }
            };
        }
    }

    /// Ensures all peers returned by the peer_iter have a slot assigned
    pub fn assign_slots_to_peers<U>(&mut self, peer_iter: U)
    where
        U: Iterator<Item = PeerId>,
    {
        for peer in peer_iter {
            self.assign_slot_if_unassigned(peer);
        }
    }

    /// Churns the slot occupied by peer.
    pub fn churn_slot(&mut self, peer: &PeerId, churn_reason: SlotChurnMetric) {
        match self.peer_slots.get(peer).cloned() {
            Some(slot) => match self.slot_metrics.get_mut(slot) {
                Some(metric_counts) => {
                    debug_assert!(!self.vacant_slots.contains(&slot),
                        "metrics_event[{}] [slot {:02}] increment {} peer {} FAILURE [vacant slots already contains this slot!]",
                            self.topic, slot, <SlotChurnMetric as Into<&'static str>>::into(churn_reason), peer
                    );
                    let churn_sum = metric_counts.churn_slot(churn_reason);
                    self.vacant_slots.insert(slot);
                    self.peer_slots.remove(peer);
                    debug!(
                        "metrics_event[{}]: [slot {:02}] increment {} peer {} SUCCESS ChurnSum[{}]",
                            self.topic, slot, <SlotChurnMetric as Into<&'static str>>::into(churn_reason), peer, churn_sum,
                    );
                },
                None => warn!(
                    "metrics_event[{}]: [slot {:02}] increment {} peer {} FAILURE [retrieving metric_counts]",
                        self.topic, slot, <SlotChurnMetric as Into<&'static str>>::into(churn_reason), peer
                ),
            },
            None => warn!(
                "metrics_event[{}]: [slot --] increment {} peer {} FAILURE [retrieving slot]",
                    self.topic, <SlotChurnMetric as Into<&'static str>>::into(churn_reason), peer
            ),
        };
    }

    /// Churns all slots in this topic that aren't already vacant (while incrementing
    /// churn_reason). Also clears the peer_slots. This loop is faster than calling
    /// churn_slot() for each peer in the topic because it minimizes redundant lookups
    /// and only traverses a vector.
    pub fn churn_all_slots(&mut self, churn_reason: SlotChurnMetric) {
        for slot in (1..self.slot_metrics.len())
            .filter(|s| !self.vacant_slots.contains(s))
            .collect::<Vec<_>>()
        {
            if let Some(metric_counts) = self.slot_metrics.get_mut(slot) {
                let previous = metric_counts.current_peer().as_ref().map(|p| **p);
                let churn_sum = metric_counts.churn_slot(churn_reason);
                self.vacant_slots.insert(slot);
                match previous {
                    Some(peer) => debug!(
                        "metrics_event[{}]: [slot {:02}] increment {} peer {} SUCCESS ChurnSum[{}]",
                        self.topic, slot, <SlotChurnMetric as Into<&'static str>>::into(churn_reason), peer, churn_sum,
                    ),
                    None => warn!(
                        "metrics_event[{}]: [slot {:02}] increment {} WARNING [current_peer not assigned with non-vacant slot!] ChurnSum[{}]",
                        self.topic, slot, <SlotChurnMetric as Into<&'static str>>::into(churn_reason), churn_sum,
                    ),
                };
            }
        }
        self.peer_slots.clear();
    }

    /// This function verifies that the TopicMetrics is synchronized perfectly with the mesh.
    /// It's useful for debugging.
    #[cfg(debug_assertions)]
    pub fn validate_mesh_slots(&self, mesh: &BTreeSet<PeerId>) -> Result<(), String> {
        let mut result = true;
        let mut errors = String::new();
        // No peers are in the peer_slots that aren't in the mesh
        for (peer, slot) in self
            .peer_slots
            .iter()
            .filter(|(peer, ..)| !mesh.contains(peer))
        {
            result = false;
            let message = format!(
                "metrics_event[{}]: [slot {:02}] peer {} exists in peer_slots but not in the mesh!\n",
                self.topic, slot, peer
            );
            errors.push_str(message.as_str());
            error!("{}", message);
        }
        // No peers are in the mesh that aren't in the peer_slots
        for peer in mesh
            .iter()
            .filter(|peer| !self.peer_slots.contains_key(peer))
        {
            result = false;
            let message = format!(
                "metrics_event[{}]: [slot --] peer {} exists in mesh but not in the peer_slots!\n",
                self.topic, peer
            );
            errors.push_str(message.as_str());
            error!("{}", message);
        }

        // vacant_slots.len() + peer_slots.len() == slot_metrics.len() + 1
        if self.vacant_slots.len() + self.peer_slots.len() + 1 != self.slot_metrics.len() {
            result = false;
            let message = format!(
                "metrics_event[{}] vacant_slots.len()[{}] + peer_slots.len()[{}] + 1 != slot_metrics.len()[{}]",
                    self.topic, self.vacant_slots.len(), self.peer_slots.len(), self.slot_metrics.len(),
            );
            errors.push_str(message.as_str());
            error!("{}", message);
        }

        if result {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub fn slot_metrics_iter(&self) -> impl Iterator<Item = &SlotMetricCounts> {
        self.slot_metrics.iter()
    }
}
