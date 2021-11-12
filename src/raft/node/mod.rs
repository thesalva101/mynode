use crossbeam_channel::Sender;

use crate::{store::Store, Error};

use super::{
    log::{Entry, Log},
    transport::{Event, Message},
    State,
};

mod candidate;
mod follower;
mod leader;

use candidate::Candidate;
use follower::Follower;
use leader::Leader;

/// The interval between leader heartbeats, in ticks.
const HEARTBEAT_INTERVAL: u64 = 1;

/// The minimum election timeout, in ticks.
const ELECTION_TIMEOUT_MIN: u64 = 8 * HEARTBEAT_INTERVAL;

/// The maximum election timeout, in ticks.
const ELECTION_TIMEOUT_MAX: u64 = 15 * HEARTBEAT_INTERVAL;

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
    state: Box<dyn State>,
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
            self.send(Some(peer), event.clone())?
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
    fn send(&self, to: Option<&str>, event: Event) -> Result<(), Error> {
        let msg = Message {
            term: self.term,
            from: Some(self.id.clone()),
            to: to.map(str::to_owned),
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
    use crate::store::KVMemory;

    pub use super::super::tests::*;
    use super::follower::tests::{follower_leader, follower_voted_for};
    use super::*;
    use crossbeam_channel::Receiver;

    pub struct NodeAsserter<'a> {
        node: &'a Node,
    }

    impl<'a> NodeAsserter<'a> {
        pub fn new(node: &'a Node) -> Self {
            Self { node }
        }

        fn log(&self) -> &'a Log {
            match self.node {
                Node::Candidate(n) => &n.log,
                Node::Follower(n) => &n.log,
                Node::Leader(n) => &n.log,
            }
        }

        pub fn applied(self, index: u64) -> Self {
            let (apply_index, _) = self.log().get_applied();
            assert_eq!(index, apply_index, "Unexpected applied index");
            self
        }

        pub fn committed(self, index: u64) -> Self {
            let (commit_index, _) = self.log().get_committed();
            assert_eq!(index, commit_index, "Unexpected committed index");
            self
        }

        pub fn last(self, index: u64) -> Self {
            let (last_index, _) = self.log().get_last();
            assert_eq!(index, last_index, "Unexpected last index");
            self
        }

        pub fn entry(self, index: u64, entry: Entry) -> Self {
            let (last_index, _) = self.log().get_last();
            assert!(index <= last_index, "Index beyond last entry");
            assert_eq!(entry, self.log().get(index).unwrap().unwrap());
            self
        }

        pub fn entries(self, entries: Vec<Entry>) -> Self {
            assert_eq!(entries, self.log().range(0..).unwrap());
            self
        }

        #[allow(clippy::wrong_self_convention)]
        pub fn is_candidate(self) -> Self {
            match self.node {
                Node::Candidate(_) => self,
                Node::Follower(_) => panic!("Expected candidate, got follower"),
                Node::Leader(_) => panic!("Expected candidate, got leader"),
            }
        }

        #[allow(clippy::wrong_self_convention)]
        pub fn is_follower(self) -> Self {
            match self.node {
                Node::Candidate(_) => panic!("Expected follower, got candidate"),
                Node::Follower(_) => self,
                Node::Leader(_) => panic!("Expected follower, got leader"),
            }
        }

        #[allow(clippy::wrong_self_convention)]
        pub fn is_leader(self) -> Self {
            match self.node {
                Node::Candidate(_) => panic!("Expected leader, got candidate"),
                Node::Follower(_) => panic!("Expected leader, got follower"),
                Node::Leader(_) => self,
            }
        }

        pub fn leader(self, leader: Option<&str>) -> Self {
            assert_eq!(
                leader.map(str::to_owned),
                match self.node {
                    Node::Candidate(_) => None,
                    Node::Follower(n) => follower_leader(n),
                    Node::Leader(_) => None,
                },
                "Unexpected leader",
            );
            self
        }

        pub fn term(self, term: u64) -> Self {
            assert_eq!(
                term,
                match self.node {
                    Node::Candidate(n) => n.term,
                    Node::Follower(n) => n.term,
                    Node::Leader(n) => n.term,
                },
                "Unexpected node term",
            );
            let (saved_term, saved_voted_for) = self.log().load_term().unwrap();
            assert_eq!(saved_term, term, "Incorrect term stored in log");
            assert_eq!(
                saved_voted_for,
                match self.node {
                    Node::Candidate(_) => None,
                    Node::Follower(n) => follower_voted_for(n),
                    Node::Leader(_) => None,
                },
                "Incorrect voted_for stored in log"
            );
            self
        }

        pub fn voted_for(self, voted_for: Option<&str>) -> Self {
            assert_eq!(
                voted_for.map(str::to_owned),
                match self.node {
                    Node::Candidate(_) => None,
                    Node::Follower(n) => follower_voted_for(n),
                    Node::Leader(_) => None,
                },
                "Unexpected voted_for"
            );
            let (_, saved_voted_for) = self.log().load_term().unwrap();
            assert_eq!(
                saved_voted_for.as_deref(),
                voted_for,
                "Unexpected voted_for saved in log"
            );
            self
        }
    }

    pub fn assert_node(node: &Node) -> NodeAsserter {
        NodeAsserter::new(node)
    }

    fn setup_rolenode() -> (RoleNode<()>, Receiver<Message>) {
        setup_rolenode_peers(vec!["b".into(), "c".into()])
    }

    fn setup_rolenode_peers(peers: Vec<String>) -> (RoleNode<()>, Receiver<Message>) {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let node = RoleNode {
            role: (),
            id: "a".into(),
            peers,
            term: 1,
            log: Log::new(KVMemory::new()).unwrap(),
            state: TestState::new().boxed(),
            sender,
        };
        (node, receiver)
    }

    #[test]
    fn new() {
        let (sender, _) = crossbeam_channel::unbounded();
        let node = Node::new(
            "a",
            vec!["b".into(), "c".into()],
            KVMemory::new(),
            TestState::new(),
            sender,
        )
        .unwrap();
        match node {
            Node::Follower(rolenode) => {
                assert_eq!(rolenode.id, "a".to_owned());
                assert_eq!(rolenode.term, 0);
                assert_eq!(rolenode.peers, vec!["b".to_owned(), "c".to_owned()]);
            }
            _ => panic!("Expected node to start as follower"),
        }
    }

    #[test]
    fn new_loads_term() {
        let (sender, _) = crossbeam_channel::unbounded();
        let store = KVMemory::new();
        Log::new(store.clone())
            .unwrap()
            .save_term(3, Some("c"))
            .unwrap();
        let node = Node::new(
            "a",
            vec!["b".into(), "c".into()],
            store,
            TestState::new(),
            sender,
        )
        .unwrap();
        match node {
            Node::Follower(rolenode) => assert_eq!(rolenode.term, 3),
            _ => panic!("Expected node to start as follower"),
        }
    }

    #[test]
    fn new_single() {
        let (sender, _) = crossbeam_channel::unbounded();
        let node = Node::new(
            "a",
            vec![],
            KVMemory::new(),
            crate::state::State::new(KVMemory::new()),
            sender,
        )
        .unwrap();
        match node {
            Node::Leader(rolenode) => {
                assert_eq!(rolenode.id, "a".to_owned());
                assert_eq!(rolenode.term, 0);
                assert!(rolenode.peers.is_empty());
            }
            _ => panic!("Expected leader"),
        }
    }

    #[test]
    fn become_role() {
        let (node, _) = setup_rolenode();
        let new = node.become_role("role").unwrap();
        assert_eq!(new.id, "a".to_owned());
        assert_eq!(new.term, 1);
        assert_eq!(new.peers, vec!["b".to_owned(), "c".to_owned()]);
        assert_eq!(new.role, "role");
    }

    #[test]
    fn broadcast() {
        let (node, rx) = setup_rolenode();
        node.broadcast(Event::Heartbeat {
            commit_index: 1,
            commit_term: 1,
        })
        .unwrap();

        for to in ["b", "c"].iter().cloned() {
            assert!(!rx.is_empty());
            assert_eq!(
                rx.recv().unwrap(),
                Message {
                    from: Some("a".into()),
                    to: Some(to.into()),
                    term: 1,
                    event: Event::Heartbeat {
                        commit_index: 1,
                        commit_term: 1
                    },
                },
            )
        }
        assert!(rx.is_empty());
    }

    #[test]
    fn normalize_message() {
        let (node, _) = setup_rolenode();
        let mut msg = Message {
            from: None,
            to: None,
            term: 0,
            event: Event::ReadState {
                call_id: vec![],
                command: vec![],
            },
        };
        assert!(node.normalize_message(&mut msg));
        assert_eq!(
            msg,
            Message {
                from: None,
                to: Some("a".into()),
                term: 1,
                event: Event::ReadState {
                    call_id: vec![],
                    command: vec![]
                },
            }
        );

        msg.to = Some("c".into());
        assert!(!node.normalize_message(&mut msg));
    }

    #[test]
    fn quorum() {
        let quorums = vec![
            (1, 1),
            (2, 2),
            (3, 2),
            (4, 3),
            (5, 3),
            (6, 4),
            (7, 4),
            (8, 5),
        ];
        for (size, quorum) in quorums.into_iter() {
            let peers: Vec<String> = (0..(size as u8 - 1))
                .map(|i| (i as char).to_string())
                .collect();
            assert_eq!(peers.len(), size as usize - 1);
            let (node, _) = setup_rolenode_peers(peers);
            assert_eq!(node.quorum(), quorum);
        }
    }

    #[test]
    fn send() {
        let (node, rx) = setup_rolenode();
        node.send(
            Some("b"),
            Event::Heartbeat {
                commit_index: 1,
                commit_term: 1,
            },
        )
        .unwrap();
        assert_messages(
            &rx,
            vec![Message {
                from: Some("a".into()),
                to: Some("b".into()),
                term: 1,
                event: Event::Heartbeat {
                    commit_index: 1,
                    commit_term: 1,
                },
            }],
        );
    }

    #[test]
    fn save_term() {
        let (mut node, _) = setup_rolenode();
        node.save_term(4, Some("b")).unwrap();
        assert_eq!(node.log.load_term().unwrap(), (4, Some("b".into())));
    }
}
