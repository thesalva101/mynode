use crate::raft;
use crate::serializer::deserialize;
use crate::serializer::serialize;
use crate::store;
use crate::Error;
use serde_derive::{Deserialize, Serialize};

/// A state machine mutation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Mutation {
    Set { key: String, value: Vec<u8> },
}

/// A state machine read.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Read {
    Get(String),
}

/// A basic key/value state machine.
#[derive(Debug)]
pub struct State {
    kv: Box<dyn store::Store>,
}

impl State {
    /// Creates a new kv state machine.
    pub fn new<S: store::Store>(kv: S) -> Self {
        State { kv: Box::new(kv) }
    }
}

impl raft::State for State {
    fn read(&self, command: Vec<u8>) -> Result<Vec<u8>, Error> {
        let read: Read = deserialize(command)?;
        match read {
            Read::Get(key) => {
                info!("Getting value for key {}", key);
                Ok(self.kv.get(&key)?.unwrap_or(serialize("")?))
            }
        }
    }

    fn mutate(&mut self, command: Vec<u8>) -> Result<Vec<u8>, Error> {
        let mutation: Mutation = deserialize(command)?;
        match mutation {
            Mutation::Set { key, value } => {
                info!("Setting {} to {:?}", key, value);
                self.kv.set(&key, value)?;
                Ok(vec![])
            }
        }
    }
}
