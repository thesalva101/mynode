use super::*;
use rand::Rng;
use std::collections::HashMap;

use super::RoleNode;

// A follower replicates state from a leader.
#[derive(Debug)]
pub struct Follower {
    /// The leader, or None if just initialized.
    leader: Option<String>,
    /// The number of ticks since the last message from the leader.
    leader_seen_ticks: u64,
    /// The timeout before triggering an election.
    leader_seen_timeout: u64,
    /// The node we voted for in the current term, if any.
    voted_for: Option<String>,
    /// Keeps track of any proxied calls to the leader (call ID to message sender).
    proxy_calls: HashMap<Vec<u8>, Option<String>>,
}

impl Follower {
    pub fn new(leader: Option<String>, voted_for: Option<String>) -> Self {
        Self {
            leader,
            leader_seen_ticks: 0,
            leader_seen_timeout: rand::thread_rng()
                .gen_range(ELECTION_TIMEOUT_MIN..ELECTION_TIMEOUT_MAX),
            voted_for,
            proxy_calls: HashMap::new(),
        }
    }
}

impl RoleNode<Follower> {
    /// Transforms the node into a candidate.
    fn become_candidate(self) -> Result<RoleNode<Candidate>, Error> {
        info!("Starting election for term {}", self.term + 1);
        let mut node = self.become_role(Candidate::new())?;
        node.save_term(node.term + 1, None)?;
        let (last_index, last_term) = node.log.get_last();
        node.broadcast(Event::SolicitVote {
            last_index,
            last_term,
        })?;
        Ok(node)
    }

    /// Checks if the message sender is the current leader
    fn is_message_sent_from_leader(&self, from: Option<&str>) -> bool {
        if let Some(leader) = self.role.leader.as_deref() {
            if let Some(claimer) = from {
                return claimer == leader;
            }
        }
        false
    }

    /// Processes a message
    pub fn step(mut self, mut msg: Message) -> Result<Node, Error> {
        if !self.normalize_message(&mut msg) {
            return Ok(self.into());
        }

        self.discover_message(&msg)?;

        if self.is_message_sent_from_leader(msg.from.as_deref()) {
            self.role.leader_seen_ticks = 0;
        }

        self.process_event(msg)
    }

    /// Discover new leaders and terms based on the received message.
    fn discover_message(&mut self, msg: &Message) -> Result<(), Error> {
        if let Some(from) = &msg.from {
            if msg.term > self.term {
                info!(
                    "Discovered a new term {}, following leader {}",
                    msg.term, from
                );
                self.save_term(msg.term, None)?;
                self.role = Follower::new(Some(from.clone()), None);
            }
            if self.role.leader.is_none() {
                info!(
                    "Discovered leader {} in current term {}, following",
                    from, self.term
                );
                self.role = Follower::new(Some(from.clone()), self.role.voted_for.clone());
            }
        }
        Ok(())
    }

    /// Process message event.
    fn process_event(mut self, msg: Message) -> Result<Node, Error> {
        match msg.event {
            Event::Heartbeat {
                commit_index,
                commit_term,
            } => {
                if self.is_message_sent_from_leader(msg.from.as_deref()) {
                    let has_committed = self.log.has(commit_index, commit_term)?;
                    self.send(
                        msg.from.as_deref(),
                        Event::ConfirmLeader {
                            commit_index,
                            has_committed,
                        },
                    )?;
                    if has_committed {
                        self.log.commit(commit_index)?;
                    }
                }
            }
            Event::SolicitVote {
                last_index,
                last_term,
            } => {
                if let Some(voted_for) = &self.role.voted_for {
                    if let Some(from) = &msg.from {
                        if voted_for != from {
                            return Ok(self.into());
                        }
                    }
                }
                let (local_last_index, local_last_term) = self.log.get_last();
                if last_term < local_last_term {
                    return Ok(self.into());
                }
                if last_term == local_last_term && last_index < local_last_index {
                    return Ok(self.into());
                }
                if let Some(from) = msg.from {
                    info!("Voting for {} in term {} election", &from, self.term);
                    self.send(Some(&from), Event::GrantVote)?;
                    self.save_term(self.term, Some(&from))?;
                    self.role.voted_for = Some(from);
                }
            }
            Event::ReplicateEntries {
                base_index,
                base_term,
                entries,
            } => {
                if self.is_message_sent_from_leader(msg.from.as_deref()) {
                    match self.log.splice(base_index, base_term, entries) {
                        Ok(last_index) => {
                            self.send(msg.from.as_deref(), Event::AcceptEntries { last_index })?
                        }
                        Err(Error::RaftBaseNotFound { .. }) => {
                            debug!("Rejecting log entries at base {}", base_index);
                            self.send(msg.from.as_deref(), Event::RejectEntries)?
                        }
                        Err(err) => return Err(err),
                    }
                }
            }
            Event::ReadState { ref call_id, .. } | Event::MutateState { ref call_id, .. } => {
                self.role.proxy_calls.insert(call_id.clone(), msg.from);
                self.send(self.role.leader.as_deref(), msg.event)?;
            }
            Event::RespondState { ref call_id, .. } | Event::RespondError { ref call_id, .. } => {
                if let Some(to) = self.role.proxy_calls.remove(call_id) {
                    self.send(to.as_deref(), msg.event)?
                } else {
                    warn!("invalid proxy respond state: {:?}", call_id);
                }
            }
            Event::ConfirmLeader { .. }
            | Event::GrantVote
            | Event::AcceptEntries { .. }
            | Event::RejectEntries => {}
        }

        Ok(self.into())
    }

    /// Processes a logical clock tick
    pub fn tick(mut self) -> Result<Node, Error> {
        while self.log.apply(&mut self.state)?.is_some() {}
        self.role.leader_seen_ticks += 1;
        if self.role.leader_seen_ticks >= self.role.leader_seen_timeout {
            Ok(self.become_candidate()?.into())
        } else {
            Ok(self.into())
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::store::KVMemory;

    use super::super::tests::{assert_messages, assert_node, TestState};
    use super::*;
    use crossbeam_channel::Receiver;

    pub fn follower_leader(node: &RoleNode<Follower>) -> Option<String> {
        node.role.leader.clone()
    }

    pub fn follower_voted_for(node: &RoleNode<Follower>) -> Option<String> {
        node.role.voted_for.clone()
    }

    fn setup() -> (RoleNode<Follower>, Receiver<Message>) {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut state = TestState::new().boxed();
        let mut log = Log::new(KVMemory::new()).unwrap();
        log.append(Entry {
            term: 1,
            command: Some(vec![0x01]),
        })
        .unwrap();
        log.append(Entry {
            term: 1,
            command: Some(vec![0x02]),
        })
        .unwrap();
        log.append(Entry {
            term: 2,
            command: Some(vec![0x03]),
        })
        .unwrap();
        log.commit(2).unwrap();
        log.apply(&mut state).unwrap();

        let mut node = RoleNode {
            id: "a".into(),
            peers: vec!["b".into(), "c".into(), "d".into(), "e".into()],
            term: 3,
            log,
            state,
            sender,
            role: Follower::new(Some("b".to_string()), None),
        };
        node.save_term(3, None).unwrap();
        (node, receiver)
    }

    #[test]
    // Heartbeat from current leader
    fn step_heartbeat() {
        let (follower, rx) = setup();
        let node = follower
            .step(Message {
                from: Some("b".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::Heartbeat {
                    commit_index: 3,
                    commit_term: 2,
                },
            })
            .unwrap();
        assert_node(&node)
            .is_follower()
            .term(3)
            .leader(Some("b"))
            .voted_for(None)
            .committed(3)
            .applied(1);
        assert_messages(
            &rx,
            vec![Message {
                from: Some("a".into()),
                to: Some("b".into()),
                term: 3,
                event: Event::ConfirmLeader {
                    commit_index: 3,
                    has_committed: true,
                },
            }],
        );
    }

    #[test]
    // Heartbeat from current leader with conflicting commit_term
    fn step_heartbeat_conflict_commit_term() {
        let (follower, rx) = setup();
        let node = follower
            .step(Message {
                from: Some("b".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::Heartbeat {
                    commit_index: 3,
                    commit_term: 3,
                },
            })
            .unwrap();
        assert_node(&node)
            .is_follower()
            .term(3)
            .leader(Some("b"))
            .voted_for(None)
            .committed(2)
            .applied(1);
        assert_messages(
            &rx,
            vec![Message {
                from: Some("a".into()),
                to: Some("b".into()),
                term: 3,
                event: Event::ConfirmLeader {
                    commit_index: 3,
                    has_committed: false,
                },
            }],
        );
    }

    #[test]
    // Heartbeat from current leader with a missing commit_index
    fn step_heartbeat_missing_commit_entry() {
        let (follower, rx) = setup();
        let node = follower
            .step(Message {
                from: Some("b".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::Heartbeat {
                    commit_index: 5,
                    commit_term: 3,
                },
            })
            .unwrap();
        assert_node(&node)
            .is_follower()
            .term(3)
            .leader(Some("b"))
            .voted_for(None)
            .committed(2)
            .applied(1);
        assert_messages(
            &rx,
            vec![Message {
                from: Some("a".into()),
                to: Some("b".into()),
                term: 3,
                event: Event::ConfirmLeader {
                    commit_index: 5,
                    has_committed: false,
                },
            }],
        );
    }

    #[test]
    // Heartbeat from fake leader
    fn step_heartbeat_fake_leader() {
        let (follower, rx) = setup();
        let node = follower
            .step(Message {
                from: Some("c".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::Heartbeat {
                    commit_index: 5,
                    commit_term: 3,
                },
            })
            .unwrap();
        assert_node(&node)
            .is_follower()
            .term(3)
            .leader(Some("b"))
            .voted_for(None)
            .committed(2)
            .applied(1);
        assert_messages(&rx, vec![]);
    }

    #[test]
    // Heartbeat when no current leader
    fn step_heartbeat_no_leader() {
        let (mut follower, rx) = setup();
        follower.role = Follower::new(None, None);
        let node = follower
            .step(Message {
                from: Some("c".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::Heartbeat {
                    commit_index: 3,
                    commit_term: 2,
                },
            })
            .unwrap();
        assert_node(&node)
            .is_follower()
            .term(3)
            .leader(Some("c"))
            .voted_for(None)
            .committed(3)
            .applied(1);
        assert_messages(
            &rx,
            vec![Message {
                from: Some("a".into()),
                to: Some("c".into()),
                term: 3,
                event: Event::ConfirmLeader {
                    commit_index: 3,
                    has_committed: true,
                },
            }],
        );
    }

    #[test]
    // Heartbeat from current leader with old commit_index
    fn step_heartbeat_old_commit_index() {
        let (follower, rx) = setup();
        let node = follower
            .step(Message {
                from: Some("b".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::Heartbeat {
                    commit_index: 1,
                    commit_term: 1,
                },
            })
            .unwrap();
        assert_node(&node)
            .is_follower()
            .term(3)
            .leader(Some("b"))
            .voted_for(None)
            .committed(2)
            .applied(1);
        assert_messages(
            &rx,
            vec![Message {
                from: Some("a".into()),
                to: Some("b".into()),
                term: 3,
                event: Event::ConfirmLeader {
                    commit_index: 1,
                    has_committed: true,
                },
            }],
        );
    }

    #[test]
    // Heartbeat for future term with other leader changes leader
    fn step_heartbeat_future_term() {
        let (follower, rx) = setup();
        let node = follower
            .step(Message {
                from: Some("c".into()),
                to: Some("a".into()),
                term: 4,
                event: Event::Heartbeat {
                    commit_index: 3,
                    commit_term: 2,
                },
            })
            .unwrap();
        assert_node(&node)
            .is_follower()
            .term(4)
            .leader(Some("c"))
            .voted_for(None);
        assert_messages(
            &rx,
            vec![Message {
                from: Some("a".into()),
                to: Some("c".into()),
                term: 4,
                event: Event::ConfirmLeader {
                    commit_index: 3,
                    has_committed: true,
                },
            }],
        );
    }

    #[test]
    // Heartbeat from past term
    fn step_heartbeat_past_term() {
        let (follower, rx) = setup();
        let node = follower
            .step(Message {
                from: Some("b".into()),
                to: Some("a".into()),
                term: 2,
                event: Event::Heartbeat {
                    commit_index: 3,
                    commit_term: 2,
                },
            })
            .unwrap();
        assert_node(&node)
            .is_follower()
            .term(3)
            .leader(Some("b"))
            .voted_for(None)
            .committed(2)
            .applied(1);
        assert_messages(&rx, vec![]);
    }

    #[test]
    // SolicitVote is granted for the first solicitor, otherwise ignored.
    fn step_solicitvote() {
        let (follower, rx) = setup();

        // The first vote request in this term yields a vote response.
        let mut node = follower
            .step(Message {
                from: Some("c".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::SolicitVote {
                    last_index: 3,
                    last_term: 2,
                },
            })
            .unwrap();
        assert_node(&node)
            .is_follower()
            .term(3)
            .leader(Some("b"))
            .voted_for(Some("c"));
        assert_messages(
            &rx,
            vec![Message {
                from: Some("a".into()),
                to: Some("c".into()),
                term: 3,
                event: Event::GrantVote,
            }],
        );

        // Another vote request from the same sender is granted.
        node = node
            .step(Message {
                from: Some("c".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::SolicitVote {
                    last_index: 3,
                    last_term: 2,
                },
            })
            .unwrap();
        assert_node(&node)
            .is_follower()
            .term(3)
            .leader(Some("b"))
            .voted_for(Some("c"));
        assert_messages(
            &rx,
            vec![Message {
                from: Some("a".into()),
                to: Some("c".into()),
                term: 3,
                event: Event::GrantVote,
            }],
        );

        // But a vote request from a different node is ignored.
        node = node
            .step(Message {
                from: Some("d".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::SolicitVote {
                    last_index: 3,
                    last_term: 2,
                },
            })
            .unwrap();
        assert_node(&node)
            .is_follower()
            .term(3)
            .leader(Some("b"))
            .voted_for(Some("c"));
        assert_messages(&rx, vec![]);
    }

    #[test]
    // GrantVote messages are ignored
    fn step_grantvote_noop() {
        let (follower, rx) = setup();
        let node = follower
            .step(Message {
                from: Some("b".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::GrantVote,
            })
            .unwrap();
        assert_node(&node).is_follower().term(3).leader(Some("b"));
        assert_messages(&rx, vec![]);
    }

    #[test]
    // SolicitVote is rejected if last_term is outdated.
    fn step_solicitvote_last_index_outdated() {
        let (follower, rx) = setup();
        let node = follower
            .step(Message {
                from: Some("c".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::SolicitVote {
                    last_index: 2,
                    last_term: 2,
                },
            })
            .unwrap();
        assert_node(&node)
            .is_follower()
            .term(3)
            .leader(Some("b"))
            .voted_for(None);
        assert_messages(&rx, vec![]);
    }

    #[test]
    // SolicitVote is rejected if last_term is outdated.
    fn step_solicitvote_last_term_outdated() {
        let (follower, rx) = setup();
        let node = follower
            .step(Message {
                from: Some("c".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::SolicitVote {
                    last_index: 3,
                    last_term: 1,
                },
            })
            .unwrap();
        assert_node(&node)
            .is_follower()
            .term(3)
            .leader(Some("b"))
            .voted_for(None);
        assert_messages(&rx, vec![]);
    }

    #[test]
    // ReplicateEntries accepts some entries at base 0 without changes
    fn step_replicateentries_base0() {
        let (follower, rx) = setup();
        let node = follower
            .step(Message {
                from: Some("b".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::ReplicateEntries {
                    base_index: 0,
                    base_term: 0,
                    entries: vec![
                        Entry {
                            term: 1,
                            command: Some(vec![0x01]),
                        },
                        Entry {
                            term: 1,
                            command: Some(vec![0x02]),
                        },
                    ],
                },
            })
            .unwrap();
        assert_node(&node).is_follower().term(3).entries(vec![
            Entry {
                term: 1,
                command: Some(vec![0x01]),
            },
            Entry {
                term: 1,
                command: Some(vec![0x02]),
            },
            Entry {
                term: 2,
                command: Some(vec![0x03]),
            },
        ]);
        assert_messages(
            &rx,
            vec![Message {
                from: Some("a".into()),
                to: Some("b".into()),
                term: 3,
                event: Event::AcceptEntries { last_index: 3 },
            }],
        );
    }

    #[test]
    // ReplicateEntries appends entries
    fn step_replicateentries_append() {
        let (follower, rx) = setup();
        let node = follower
            .step(Message {
                from: Some("b".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::ReplicateEntries {
                    base_index: 3,
                    base_term: 2,
                    entries: vec![
                        Entry {
                            term: 3,
                            command: Some(vec![0x04]),
                        },
                        Entry {
                            term: 3,
                            command: Some(vec![0x05]),
                        },
                    ],
                },
            })
            .unwrap();
        assert_node(&node).is_follower().term(3).entries(vec![
            Entry {
                term: 1,
                command: Some(vec![0x01]),
            },
            Entry {
                term: 1,
                command: Some(vec![0x02]),
            },
            Entry {
                term: 2,
                command: Some(vec![0x03]),
            },
            Entry {
                term: 3,
                command: Some(vec![0x04]),
            },
            Entry {
                term: 3,
                command: Some(vec![0x05]),
            },
        ]);
        assert_messages(
            &rx,
            vec![Message {
                from: Some("a".into()),
                to: Some("b".into()),
                term: 3,
                event: Event::AcceptEntries { last_index: 5 },
            }],
        );
    }

    #[test]
    // ReplicateEntries accepts partially overlapping entries
    fn step_replicateentries_partial_overlap() {
        let (follower, rx) = setup();
        let node = follower
            .step(Message {
                from: Some("b".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::ReplicateEntries {
                    base_index: 1,
                    base_term: 1,
                    entries: vec![
                        Entry {
                            term: 1,
                            command: Some(vec![0x02]),
                        },
                        Entry {
                            term: 2,
                            command: Some(vec![0x03]),
                        },
                        Entry {
                            term: 3,
                            command: Some(vec![0x04]),
                        },
                    ],
                },
            })
            .unwrap();
        assert_node(&node).is_follower().term(3).entries(vec![
            Entry {
                term: 1,
                command: Some(vec![0x01]),
            },
            Entry {
                term: 1,
                command: Some(vec![0x02]),
            },
            Entry {
                term: 2,
                command: Some(vec![0x03]),
            },
            Entry {
                term: 3,
                command: Some(vec![0x04]),
            },
        ]);
        assert_messages(
            &rx,
            vec![Message {
                from: Some("a".into()),
                to: Some("b".into()),
                term: 3,
                event: Event::AcceptEntries { last_index: 4 },
            }],
        );
    }

    #[test]
    // ReplicateEntries replaces conflicting entries
    fn step_replicateentries_replace() {
        let (follower, rx) = setup();
        let node = follower
            .step(Message {
                from: Some("b".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::ReplicateEntries {
                    base_index: 2,
                    base_term: 1,
                    entries: vec![
                        Entry {
                            term: 3,
                            command: Some(vec![0x04]),
                        },
                        Entry {
                            term: 3,
                            command: Some(vec![0x05]),
                        },
                    ],
                },
            })
            .unwrap();
        assert_node(&node).is_follower().term(3).entries(vec![
            Entry {
                term: 1,
                command: Some(vec![0x01]),
            },
            Entry {
                term: 1,
                command: Some(vec![0x02]),
            },
            Entry {
                term: 3,
                command: Some(vec![0x04]),
            },
            Entry {
                term: 3,
                command: Some(vec![0x05]),
            },
        ]);
        assert_messages(
            &rx,
            vec![Message {
                from: Some("a".into()),
                to: Some("b".into()),
                term: 3,
                event: Event::AcceptEntries { last_index: 4 },
            }],
        );
    }

    #[test]
    // ReplicateEntries replaces partially conflicting entries
    fn step_replicateentries_replace_partial() {
        let (follower, rx) = setup();
        let node = follower
            .step(Message {
                from: Some("b".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::ReplicateEntries {
                    base_index: 2,
                    base_term: 1,
                    entries: vec![
                        Entry {
                            term: 2,
                            command: Some(vec![0x03]),
                        },
                        Entry {
                            term: 3,
                            command: Some(vec![0x04]),
                        },
                    ],
                },
            })
            .unwrap();
        assert_node(&node).is_follower().term(3).entries(vec![
            Entry {
                term: 1,
                command: Some(vec![0x01]),
            },
            Entry {
                term: 1,
                command: Some(vec![0x02]),
            },
            Entry {
                term: 2,
                command: Some(vec![0x03]),
            },
            Entry {
                term: 3,
                command: Some(vec![0x04]),
            },
        ]);
        assert_messages(
            &rx,
            vec![Message {
                from: Some("a".into()),
                to: Some("b".into()),
                term: 3,
                event: Event::AcceptEntries { last_index: 4 },
            }],
        );
    }

    #[test]
    // ReplicateEntries rejects missing base index
    fn step_replicateentries_reject_missing_base_index() {
        let (follower, rx) = setup();
        let node = follower
            .step(Message {
                from: Some("b".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::ReplicateEntries {
                    base_index: 5,
                    base_term: 2,
                    entries: vec![Entry {
                        term: 3,
                        command: Some(vec![0x04]),
                    }],
                },
            })
            .unwrap();
        assert_node(&node).is_follower().term(3).entries(vec![
            Entry {
                term: 1,
                command: Some(vec![0x01]),
            },
            Entry {
                term: 1,
                command: Some(vec![0x02]),
            },
            Entry {
                term: 2,
                command: Some(vec![0x03]),
            },
        ]);
        assert_messages(
            &rx,
            vec![Message {
                from: Some("a".into()),
                to: Some("b".into()),
                term: 3,
                event: Event::RejectEntries,
            }],
        );
    }

    #[test]
    // ReplicateEntries rejects conflicting base term
    fn step_replicateentries_reject_missing_base_term() {
        let (follower, rx) = setup();
        let node = follower
            .step(Message {
                from: Some("b".into()),
                to: Some("a".into()),
                term: 3,
                event: Event::ReplicateEntries {
                    base_index: 1,
                    base_term: 2,
                    entries: vec![Entry {
                        term: 3,
                        command: Some(vec![0x04]),
                    }],
                },
            })
            .unwrap();
        assert_node(&node).is_follower().term(3).entries(vec![
            Entry {
                term: 1,
                command: Some(vec![0x01]),
            },
            Entry {
                term: 1,
                command: Some(vec![0x02]),
            },
            Entry {
                term: 2,
                command: Some(vec![0x03]),
            },
        ]);
        assert_messages(
            &rx,
            vec![Message {
                from: Some("a".into()),
                to: Some("b".into()),
                term: 3,
                event: Event::RejectEntries,
            }],
        );
    }

    #[test]
    // ReadState and MutateState are proxied, as are the responses
    fn step_readstate_mutatestate_respond() {
        let calls = vec![
            Event::MutateState {
                call_id: vec![0x02],
                command: vec![0x02],
            },
            Event::ReadState {
                call_id: vec![0x01],
                command: vec![0x01],
            },
        ];
        let responses = vec![
            Event::RespondError {
                call_id: vec![],
                error: "b00m".into(),
            },
            Event::RespondState {
                call_id: vec![],
                response: vec![0xaf],
            },
        ];
        let (follower, rx) = setup();
        let mut node = Node::Follower(follower);
        for call in calls.into_iter() {
            for mut response in responses.clone().into_iter() {
                node = node
                    .step(Message {
                        from: None,
                        to: None,
                        term: 0,
                        event: call.clone(),
                    })
                    .unwrap();
                assert_node(&node).is_follower().term(3).leader(Some("b"));
                assert_messages(
                    &rx,
                    vec![Message {
                        from: Some("a".into()),
                        to: Some("b".into()),
                        term: 3,
                        event: call.clone(),
                    }],
                );
                match response {
                    Event::RespondError {
                        ref mut call_id, ..
                    }
                    | Event::RespondState {
                        ref mut call_id, ..
                    } => *call_id = call.call_id().unwrap(),
                    _ => {}
                }
                // Multiple responses should only be proxied once.
                for _ in 0..3 {
                    node = node
                        .step(Message {
                            from: Some("b".into()),
                            to: Some("a".into()),
                            term: 3,
                            event: response.clone(),
                        })
                        .unwrap();
                }
                assert_messages(
                    &rx,
                    vec![Message {
                        from: Some("a".into()),
                        to: None,
                        term: 3,
                        event: response,
                    }],
                );
            }
        }
    }

    #[test]
    fn tick() {
        let (follower, rx) = setup();
        let timeout = follower.role.leader_seen_timeout;
        let peers = follower.peers.clone();
        let mut node = Node::Follower(follower);

        // Make sure heartbeats reset election timeout
        assert!(timeout > 0);
        for i in 0..(3 * timeout) {
            let applied = if i > 0 { 2 } else { 1 };
            assert_node(&node)
                .is_follower()
                .term(3)
                .leader(Some("b"))
                .applied(applied);
            node = node.tick().unwrap();
            node = node
                .step(Message {
                    from: Some("b".into()),
                    to: Some("a".into()),
                    term: 3,
                    event: Event::Heartbeat {
                        commit_index: 2,
                        commit_term: 1,
                    },
                })
                .unwrap();
            assert_messages(
                &rx,
                vec![Message {
                    from: Some("a".into()),
                    to: Some("b".into()),
                    term: 3,
                    event: Event::ConfirmLeader {
                        commit_index: 2,
                        has_committed: true,
                    },
                }],
            )
        }

        for _ in 0..timeout {
            assert_node(&node).is_follower().term(3).leader(Some("b"));
            node = node.tick().unwrap();
        }
        assert_node(&node).is_candidate().term(4);

        for to in peers.into_iter() {
            assert!(!rx.is_empty());
            assert_eq!(
                rx.recv().unwrap(),
                Message {
                    from: Some("a".into()),
                    to: Some(to),
                    term: 4,
                    event: Event::SolicitVote {
                        last_index: 3,
                        last_term: 2
                    },
                }
            )
        }
    }
}
