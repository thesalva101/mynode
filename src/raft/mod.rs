mod log;
mod state;
mod transport;

use std::collections::HashMap;

use crossbeam_channel::{Receiver, Sender};
pub use state::State;

use crate::{store, Error};

use self::transport::{Event, Message, Transport};

/// The duration of a Raft tick, which is the unit of time for e.g.
/// heartbeat intervals and election timeouts.
const TICK: std::time::Duration = std::time::Duration::from_millis(100);

#[derive(Clone)]
pub struct Raft {
    call_tx: Sender<(Event, Sender<Event>)>,
    join_rx: Receiver<Result<(), Error>>,
}

impl Raft {
    /// Starts a new Raft state machine in a separate thread.
    pub fn start<S, L, T>(
        id: &str,
        peers: Vec<String>,
        state: S,
        store: L,
        transport: T,
    ) -> Result<Raft, Error>
    where
        S: State,
        L: store::Store,
        T: Transport,
    {
        let ticker = crossbeam_channel::tick(TICK);

        let inbound_rx = transport.receiver();
        let (outbound_tx, outbound_rx) = crossbeam_channel::unbounded::<(Event, Sender<Message>)>();
        let (call_tx, call_rx) = crossbeam_channel::unbounded::<(Event, Sender<Event>)>();
        let (join_tx, join_rx) = crossbeam_channel::unbounded();

        let mut response_txs: HashMap<Vec<u8>, Sender<Event>> = HashMap::new();
        // let mut node = Node::new(id, peers, store, state, outbound_tx)?;

        Ok(Raft { call_tx, join_rx })
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crossbeam_channel::Receiver;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Debug)]
    pub struct TestState {
        commands: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl TestState {
        pub fn new() -> Self {
            Self {
                commands: Arc::new(Mutex::new(Vec::new())),
            }
        }

        pub fn boxed(&self) -> Box<State> {
            Box::new(self.clone())
        }

        pub fn list(&self) -> Vec<Vec<u8>> {
            self.commands.lock().unwrap().clone()
        }
    }

    impl State for TestState {
        // Appends the command to the internal commands list, and
        // returns the command prefixed with a 0xff byte.
        fn mutate(&mut self, command: Vec<u8>) -> Result<Vec<u8>, Error> {
            if command.len() != 1 {
                return Err(Error::Value("Mutation payload must be 1 byte".into()));
            }
            self.commands.lock()?.push(command.clone());
            Ok(vec![0xff, command[0]])
        }

        // Reads the command in the internal commands list at the index
        // given by the read command (1-based). Returns the stored command prefixed by
        // 0xbb, or 0xbb 0x00 if not found.
        fn read(&self, command: Vec<u8>) -> Result<Vec<u8>, Error> {
            if command.len() != 1 {
                return Err(Error::Value("Read payload must be 1 byte".into()));
            }
            let index = command[0] as usize;
            Ok(vec![
                0xbb,
                self.commands
                    .lock()?
                    .get(index - 1)
                    .map(|c| c[0])
                    .unwrap_or(0x00),
            ])
        }
    }
}
