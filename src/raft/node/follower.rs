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
    fn is_message_sent_from_leader(&self, msg: &Message) -> bool {
        if let Some(leader) = self.role.leader.as_deref() {
            if let Some(claimer) = msg.from.as_deref() {
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

        if self.is_message_sent_from_leader(&msg) {
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
                if self.is_message_sent_from_leader(&msg) {
                    let has_committed = self.log.has(commit_index, commit_term)?;
                    self.send(
                        msg.from,
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
            } => todo!(),
            Event::ReplicateEntries {
                base_index,
                base_term,
                entries,
            } => todo!(),
            Event::ReadState { call_id, command } => todo!(),
            Event::MutateState { call_id, command } => todo!(),
            Event::RespondState { ref call_id, .. } | Event::RespondError { ref call_id, .. } => {
                if let Some(to) = self.role.proxy_calls.remove(call_id) {
                    self.send(to, msg.event)?
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
