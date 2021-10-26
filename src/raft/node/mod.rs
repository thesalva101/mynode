use crossbeam_channel::Sender;

use crate::{store::Store, Error};

use super::{
    log::{Entry, Log},
    transport::{Event, Message},
    State,
};

mod follower;

use follower::Follower;

/// The interval between leader heartbeats, in ticks.
const HEARTBEAT_INTERVAL: u64 = 1;

/// The minimum election timeout, in ticks.
const ELECTION_TIMEOUT_MIN: u64 = 8 * HEARTBEAT_INTERVAL;

/// The maximum election timeout, in ticks.
const ELECTION_TIMEOUT_MAX: u64 = 15 * HEARTBEAT_INTERVAL;

#[derive(Debug)]
struct Candidate;

impl Candidate {
    pub fn new() -> Self {
        todo!("impl")
    }
}

impl RoleNode<Candidate> {
    pub fn step(mut self, mut msg: Message) -> Result<Node, Error> {
        todo!("impl")
    }

    pub fn tick(self) -> Result<Node, Error> {
        todo!("impl")
    }
}

#[derive(Debug)]
struct Leader;

impl Leader {
    pub fn new(peers: Vec<String>, last_index: u64) -> Self {
        todo!("impl")
    }
}

impl RoleNode<Leader> {
    pub fn step(mut self, mut msg: Message) -> Result<Node, Error> {
        todo!("impl")
    }

    pub fn tick(self) -> Result<Node, Error> {
        todo!("impl")
    }
}

/// The local Raft node state machine.
#[derive(Debug)]
pub enum Node {
    Candidate(RoleNode<Candidate>),
    Follower(RoleNode<Follower>),
    Leader(RoleNode<Leader>),
}

impl Node {
    /// Creates a new Raft node, starting as a follower, or leader if no peers.
    pub fn new<L: Store, S: State>(
        id: &str,
        peers: Vec<String>,
        log_store: L,
        state: S,
        sender: Sender<Message>,
    ) -> Result<Node, Error> {
        let log = Log::new(log_store)?;
        let (term, voted_for) = log.load_term()?;
        let node = RoleNode {
            id: id.into(),
            peers,
            term,
            log,
            state: Box::new(state),
            sender,
            role: Follower::new(None, voted_for),
        };
        if node.peers.is_empty() {
            info!("No peers specified, starting as leader");
            let (last_index, _) = node.log.get_last();
            Ok(node.become_role(Leader::new(vec![], last_index))?.into())
        } else {
            Ok(node.into())
        }
    }

    /// Processes a message.
    pub fn step(self, msg: Message) -> Result<Node, Error> {
        debug!("Stepping {:?}", msg);
        match self {
            Node::Candidate(n) => n.step(msg),
            Node::Follower(n) => n.step(msg),
            Node::Leader(n) => n.step(msg),
        }
    }

    /// Moves time forward by a tick.
    pub fn tick(self) -> Result<Node, Error> {
        match self {
            Node::Candidate(n) => n.tick(),
            Node::Follower(n) => n.tick(),
            Node::Leader(n) => n.tick(),
        }
    }
}

impl From<RoleNode<Candidate>> for Node {
    fn from(rn: RoleNode<Candidate>) -> Self {
        Node::Candidate(rn)
    }
}

impl From<RoleNode<Follower>> for Node {
    fn from(rn: RoleNode<Follower>) -> Self {
        Node::Follower(rn)
    }
}

impl From<RoleNode<Leader>> for Node {
    fn from(rn: RoleNode<Leader>) -> Self {
        Node::Leader(rn)
    }
}

// A Raft node with role R
#[derive(Debug)]
pub struct RoleNode<R> {
    id: String,
    peers: Vec<String>,
    term: u64,
    log: Log,
    state: Box<State>,
    sender: Sender<Message>,
    role: R,
}

impl<R> RoleNode<R> {
    /// Transforms the node into another role.
    fn become_role<T>(self, role: T) -> Result<RoleNode<T>, Error> {
        Ok(RoleNode {
            id: self.id,
            peers: self.peers,
            term: self.term,
            log: self.log,
            state: self.state,
            sender: self.sender,
            role,
        })
    }

    /// Broadcasts an event to all peers.
    fn broadcast(&self, event: Event) -> Result<(), Error> {
        for peer in self.peers.iter() {
            self.send(Some(peer.into()), event.clone())?
        }
        Ok(())
    }

    /// Normalizes and validates a message, ensuring it is addressed
    /// to the local node and term. On any errors it emits a warning and
    /// returns false.
    fn normalize_message(&self, msg: &mut Message) -> bool {
        msg.normalize(&self.id, self.term);
        if let Err(err) = msg.validate(&self.id, self.term) {
            warn!("{}", err);
            false
        } else {
            true
        }
    }

    /// Sends an event to a peer, or local caller if None
    fn send(&self, to: Option<String>, event: Event) -> Result<(), Error> {
        let msg = Message {
            term: self.term,
            from: Some(self.id.clone()),
            to,
            event,
        };
        debug!("Sending {:?}", msg);
        Ok(self.sender.send(msg)?)
    }

    /// Returns the quorum size of the cluster.
    fn quorum(&self) -> u64 {
        (self.peers.len() as u64 + 1) / 2 + 1
    }

    /// Updates the current term and stores it in the log
    fn save_term(&mut self, term: u64, voted_for: Option<&str>) -> Result<(), Error> {
        self.log.save_term(term, voted_for)?;
        self.term = term;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    pub use super::super::tests::*;
    // use super::follower::tests::{follower_leader, follower_voted_for};
    use super::*;
    use crossbeam_channel::Receiver;
}
