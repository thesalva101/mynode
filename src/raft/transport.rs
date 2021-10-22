use crossbeam_channel::Receiver;

use crate::Error;

use super::log::Entry;

/// A transport for communication between a Raft node and its peers.
pub trait Transport: 'static + Sync + Send {
    /// Returns a channel for receiving inbound messages
    fn receiver(&self) -> Receiver<Message>;

    /// Sends a message to a peer.
    fn send(&self, msg: Message) -> Result<(), Error>;
}

/// A message passed between Raft nodes.
#[derive(Debug, PartialEq)]
pub struct Message {
    /// The current term of the sender.
    pub term: u64,

    /// The ID of the sending node, or None if local sender.
    pub from: Option<String>,

    /// The ID of the receiving node, or None if local receiver.
    pub to: Option<String>,

    /// The message event.
    pub event: Event,
}

impl Message {
    /// Normalizes a message by setting to and term for local messages
    pub fn normalize(&mut self, node_id: &str, term: u64) {
        self.to = Some(node_id.into());
        if self.term == 0 {
            self.term = term;
        }
    }

    /// Validates a message against a receiving node
    pub fn validate(&self, node_id: &str, term: u64) -> Result<(), Error> {
        // Don't allow local messages without call ID
        if self.from.is_none() && self.event.call_id().is_none() {
            return Err(Error::Network(format!(
                "Received local non-call event: {:?}",
                self.event
            )));
        }

        // Ignore messages from past terms
        if self.term < term {
            return Err(Error::Network(format!(
                "Ignoring message from stale term {}",
                self.term
            )));
        }

        // Ignore mesages to peers or local client
        if let Some(to) = &self.to {
            if to != node_id {
                return Err(Error::Network(format!(
                    "Ignoring message for other node {}",
                    to
                )));
            }
        } else {
            return Err(Error::Network("Ignoring message for local client".into()));
        }

        Ok(())
    }
}

/// An Event contained within messages.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Leaders send periodic heartbeats to its followers.
    Heartbeat { commit_index: u64, commit_term: u64 },
    /// Followers confirm loyalty to leader after heartbeats
    ConfirmLeader {
        /// The commit_index of the original leader heartbeat, to confirm
        /// read requests.
        commit_index: u64,
        /// If false the follower does not have the entry at commit_index
        /// and wants the leader to replicate it.
        has_commited: bool,
    },
    /// Candidates solicit votes from all other peers
    SolicitVote {
        /// The index of the candidate's last stored log entry
        last_index: u64,
        /// The term of the candidate's last stored log entry
        last_term: u64,
    },
    /// Followers may grant votes to candidates
    GrantVote,
    /// Leaders replicate a set of log entries to followers
    ReplicateEntries {
        /// The index of the log entry immediately preceding the submitted commands.
        base_index: u64,
        /// The term of the log entry immediately preceding the submitted commands.
        base_term: u64,
        /// Commands to replicate.
        entries: Vec<Entry>,
    },
    /// Followers may accept a set of log entries from a leader
    AcceptEntries {
        /// The index of the last log entry
        last_index: u64,
    },
    /// Followers may also reject a set of log entries from a leader
    RejectEntries,
    /// Reads from the state machine
    ReadState {
        /// The call ID
        call_id: Vec<u8>,
        /// The state machine command
        command: Vec<u8>,
    },
    /// Mutates the state machine
    MutateState {
        /// The call ID
        call_id: Vec<u8>,
        /// The state machine command
        command: Vec<u8>,
    },
    /// The response of a state machine command
    RespondState {
        /// The call ID
        call_id: Vec<u8>,
        /// The command response
        respone: Vec<u8>,
    },
    /// Reads from the state machine
    RespondError {
        /// The call ID
        call_id: Vec<u8>,
        /// The response error
        error: String,
    },
}

impl Event {
    pub fn call_id(&self) -> Option<Vec<u8>> {
        match self {
            Event::ReadState { call_id, .. }
            | Event::MutateState { call_id, .. }
            | Event::RespondState { call_id, .. }
            | Event::RespondError { call_id, .. } => Some(call_id.clone()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize() {
        let mut msg = Message {
            from: None,
            to: None,
            term: 0,
            event: Event::Heartbeat {
                commit_index: 1,
                commit_term: 1,
            },
        };

        msg.normalize("alice", 3);

        assert_eq!(
            msg,
            Message {
                from: None,
                to: Some("alice".into()),
                term: 3,
                event: Event::Heartbeat {
                    commit_index: 1,
                    commit_term: 1,
                }
            }
        )
    }

    #[test]
    fn normalize_peer() {
        let mut msg = Message {
            from: Some("bob".into()),
            to: Some("alice".into()),
            term: 3,
            event: Event::Heartbeat {
                commit_index: 1,
                commit_term: 1,
            },
        };

        msg.normalize("charlie", 9);

        assert_eq!(
            msg,
            Message {
                from: Some("bob".into()),
                to: Some("charlie".into()),
                term: 3,
                event: Event::Heartbeat {
                    commit_index: 1,
                    commit_term: 1,
                }
            }
        )
    }

    #[test]
    fn validate() {
        let event = Event::Heartbeat {
            commit_index: 1,
            commit_term: 1,
        };

        // Errors on stale term
        assert!(Message {
            from: Some("bob".into()),
            to: Some("alice".into()),
            term: 2,
            event: event.clone(),
        }
        .validate("alice", 3)
        .is_err());

        // Errors on no receiver
        assert!(Message {
            from: Some("bob".into()),
            to: None,
            term: 3,
            event: event.clone()
        }
        .validate("alice", 3)
        .is_err());

        // Errors on other receiver
        assert!(Message {
            from: Some("bob".into()),
            to: Some("charlie".into()),
            term: 3,
            event: event.clone(),
        }
        .validate("alice", 3)
        .is_err());
    }
}
