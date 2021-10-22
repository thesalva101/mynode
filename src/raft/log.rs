use serde_derive::{Deserialize, Serialize};

use crate::{
    serializer::{deserialize, serialize},
    store::Store,
    Error,
};

use super::State;

/// A replicated log entry
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// The term in which the entry was added
    pub term: u64,
    /// The state machine command. None is used to commit noops during leader election.
    pub command: Option<Vec<u8>>,
}

/// The replicated Raft Log
#[derive(Debug)]
pub struct Log {
    /// The underlying kv store
    kv: Box<dyn Store>,
    /// The index of the last stored entry.
    last_index: u64,
    /// The term of the last stored entry.
    last_term: u64,
    /// The last entry known to be committed. Not persisted,
    /// since leaders will determine this when they're elected.
    commit_index: u64,
    /// The term of the last committed entry.
    commit_term: u64,
    /// The last entry applied to the state machine. This is
    /// persisted, since the state machine is also persisted.
    apply_index: u64,
    /// The term of the last applied entry.
    apply_term: u64,
}

impl Log {
    pub fn new<S: Store>(store: S) -> Result<Self, Error> {
        let apply_index = match store.get("apply_index")? {
            Some(raw_apply_index) => deserialize(raw_apply_index)?,
            None => 0,
        };

        let (commit_index, commit_term) = match store.get(&apply_index.to_string())? {
            Some(raw_entry) => (apply_index, deserialize::<Entry>(raw_entry)?.term),
            None if apply_index == 0 => (0, 0),
            None => {
                return Err(Error::Internal(format!(
                    "Applied Entry {} not found",
                    apply_index
                )))
            }
        };
        let apply_term = commit_term;

        let (last_index, last_term) = Self::get_last_index_and_term(&store)?;

        Ok(Self {
            kv: Box::new(store),
            last_index,
            last_term,
            commit_index,
            commit_term,
            apply_index,
            apply_term,
        })
    }

    /// Appends an entry in the log
    pub fn append(&mut self, entry: Entry) -> Result<u64, Error> {
        debug!("Appending log entry: {}: {:?}", self.last_index + 1, entry);
        let index = self.last_index + 1;
        self.last_index = index;
        self.last_term = entry.term;
        self.kv.set(&index.to_string(), serialize(entry)?)?;
        Ok(index)
    }

    pub fn commit(&mut self, mut index: u64) -> Result<u64, Error> {
        index = std::cmp::min(index, self.last_index);
        index = std::cmp::max(index, self.commit_index);

        if index != self.commit_index {
            if let Some(entry) = self.get(index)? {
                debug!("Committing log entry {}", index);
                self.commit_index = index;
                self.commit_term = entry.term;
            } else {
                return Err(Error::Internal(format!(
                    "Entry at commit index {} does not exist",
                    index
                )));
            }
        }

        Ok(index)
    }

    /// Apply the next committed entry to the state machine, if any.
    /// Returns the applied entry index and output, or None if no entry.
    pub fn apply(&mut self, state: &mut Box<dyn State>) -> Result<Option<(u64, Vec<u8>)>, Error> {
        if self.apply_index >= self.commit_index {
            return Ok(None);
        }

        let mut output = vec![];
        if let Some(entry) = self.get(self.apply_index + 1)? {
            debug!("Applying log entry: {}: {:?}", self.apply_index + 1, entry);
            if let Some(command) = entry.command {
                output = state.mutate(command)?;
            }
            self.apply_index += 1;
            self.apply_term = entry.term;
        }

        self.kv.set("apply_index", serialize(self.apply_index)?)?;
        Ok(Some((self.apply_index, output)))
    }

    /// Fetches an entry at an index
    pub fn get(&self, index: u64) -> Result<Option<Entry>, Error> {
        if let Some(value) = self.kv.get(&index.to_string())? {
            Ok(Some(deserialize(value)?))
        } else {
            Ok(None)
        }
    }

    /// Fetches the last applied index and term
    pub fn get_applied(&self) -> (u64, u64) {
        (self.apply_index, self.apply_term)
    }

    /// Fetches the last committed index and term
    pub fn get_committed(&self) -> (u64, u64) {
        (self.commit_index, self.commit_term)
    }

    /// Fetches the last stored index and term
    pub fn get_last(&self) -> (u64, u64) {
        (self.last_index, self.last_term)
    }

    /// Checks if the log contains an entry
    pub fn has(&self, index: u64, term: u64) -> Result<bool, Error> {
        if index == 0 && term == 0 {
            return Ok(true);
        }
        match self.get(index)? {
            Some(ref entry) => Ok(entry.term == term),
            None => Ok(false),
        }
    }

    fn get_last_index_and_term<S: Store>(store: &S) -> Result<(u64, u64), Error> {
        let mut last_index = 0;
        let mut last_term = 0;

        for i in 1..std::u64::MAX {
            if let Some(raw_entry) = store.get(&i.to_string())? {
                let entry = deserialize::<Entry>(raw_entry)?;
                last_index = i;
                last_term = entry.term;
            } else {
                break;
            }
        }

        Ok((last_index, last_term))
    }
}

#[cfg(test)]
use std::{println as info, println as warn, println as debug};

#[cfg(test)]
mod tests {
    use crate::store;

    use super::super::tests::TestState;
    use super::*;

    fn setup() -> (Log, store::KVMemory) {
        let store = store::KVMemory::new();
        let log = Log::new(store.clone()).unwrap();
        (log, store)
    }

    fn setup_appends(l: &mut Log) {
        l.append(Entry {
            term: 1,
            command: Some(vec![0x01]),
        })
        .unwrap();
        l.append(Entry {
            term: 2,
            command: None,
        })
        .unwrap();
        l.append(Entry {
            term: 2,
            command: Some(vec![0x03]),
        })
        .unwrap();
    }

    #[test]
    fn new() {
        let (l, _) = setup();
        assert_eq!((0, 0), l.get_last());
        assert_eq!((0, 0), l.get_committed());
        assert_eq!((0, 0), l.get_applied());
        assert_eq!(None, l.get(1).unwrap());
    }

    #[test]
    fn append() {
        let (mut l, _) = setup();
        assert_eq!(Ok(None), l.get(1));
        assert_eq!(Ok(None), l.get(9));

        assert_eq!(
            Ok(1),
            l.append(Entry {
                term: 7,
                command: Some(vec![0x01]),
            })
        );
        assert_eq!(
            Ok(2),
            l.append(Entry {
                term: 9,
                command: Some(vec![0x02]),
            })
        );

        let entry3 = Entry {
            term: 10,
            command: Some(vec![0x03]),
        };
        assert_eq!(Ok(3), l.append(entry3.clone()));
        assert_eq!((3, 10), l.get_last());
        assert_eq!((0, 0), l.get_committed());
        assert_eq!((0, 0), l.get_applied());
        assert_eq!(Ok(Some(entry3)), l.get(3));
    }

    #[test]
    fn append_none_command() {
        let (mut l, _) = setup();
        assert_eq!(
            Ok(1),
            l.append(Entry {
                term: 3,
                command: None
            })
        );
        assert_eq!(
            Ok(Some(Entry {
                term: 3,
                command: None
            })),
            l.get(1)
        );
    }

    #[test]
    fn append_persistence() {
        let (mut l, store) = setup();
        setup_appends(&mut l);
        assert_eq!((0, 0), l.get_applied());

        assert_eq!((0, 0), l.get_committed());
        assert_eq!((3, 2), l.get_last());

        let l = Log::new(store).unwrap();
        assert_eq!(
            Ok(Some(Entry {
                term: 1,
                command: Some(vec![0x01])
            })),
            l.get(1)
        );
        assert_eq!(
            Ok(Some(Entry {
                term: 2,
                command: None
            })),
            l.get(2)
        );
        assert_eq!(
            Ok(Some(Entry {
                term: 2,
                command: Some(vec![0x03])
            })),
            l.get(3)
        );
        assert_eq!((0, 0), l.get_applied());
        assert_eq!((0, 0), l.get_committed());
        assert_eq!((3, 2), l.get_last());
    }

    #[test]
    fn get() {
        let (mut l, _) = setup();
        assert_eq!(Ok(None), l.get(1));

        l.append(Entry {
            term: 3,
            command: Some(vec![0x01]),
        })
        .unwrap();
        assert_eq!(
            Ok(Some(Entry {
                term: 3,
                command: Some(vec![0x01])
            })),
            l.get(1)
        );
        assert_eq!(Ok(None), l.get(2));
    }

    #[test]
    fn commit() {
        let (mut l, _) = setup();
        setup_appends(&mut l);
        assert_eq!(Ok(3), l.commit(3));
        assert_eq!((3, 2), l.get_committed());
    }

    #[test]
    fn commit_beyond() {
        let (mut l, _) = setup();
        setup_appends(&mut l);
        assert_eq!(Ok(3), l.commit(999));
        assert_eq!((3, 2), l.get_committed());
    }

    #[test]
    fn commit_partial() {
        let (mut l, _) = setup();
        setup_appends(&mut l);
        assert_eq!(Ok(2), l.commit(2));
        assert_eq!((2, 2), l.get_committed());
    }

    #[test]
    fn commit_reduce() {
        let (mut l, _) = setup();
        setup_appends(&mut l);
        assert_eq!(Ok(1), l.commit(1));
        assert_eq!((1, 1), l.get_committed());

        assert_eq!(Ok(2), l.commit(2));
        assert_eq!((2, 2), l.get_committed());

        assert_eq!(Ok(3), l.commit(3));
        assert_eq!((3, 2), l.get_committed());

        assert_eq!(Ok(3), l.commit(2));
        assert_eq!((3, 2), l.get_committed());
    }

    #[test]
    fn commit_zero() {
        let (mut l, _) = setup();
        assert_eq!(Ok(0), l.commit(0));
        assert_eq!((0, 0), l.get_committed());

        assert_eq!(Ok(0), l.commit(5));
        assert_eq!((0, 0), l.get_committed());
    }

    #[test]
    fn apply() {
        let (mut l, store) = setup();
        setup_appends(&mut l);
        l.commit(3).unwrap();

        let state = TestState::new();
        assert_eq!(Ok(Some((1, vec![0xff, 0x01]))), l.apply(&mut state.boxed()));
        assert_eq!((1, 1), l.get_applied());
        assert_eq!(vec![vec![0x01]], state.list());

        assert_eq!(Ok(Some((2, vec![]))), l.apply(&mut state.boxed()));
        assert_eq!((2, 2), l.get_applied());
        assert_eq!(vec![vec![0x01]], state.list());

        assert_eq!(Ok(Some((3, vec![0xff, 0x03]))), l.apply(&mut state.boxed()));
        assert_eq!((3, 2), l.get_applied());
        assert_eq!(vec![vec![0x01], vec![0x03]], state.list());

        // The last applied entry should be persisted, and also used for last committed
        let l = Log::new(store).unwrap();
        assert_eq!((3, 2), l.get_last());
        assert_eq!((3, 2), l.get_committed());
        assert_eq!((3, 2), l.get_applied());
    }

    #[test]
    fn apply_committed_only() {
        let (mut l, store) = setup();
        setup_appends(&mut l);
        l.commit(2).unwrap();

        let state = TestState::new();
        l.apply(&mut state.boxed()).unwrap();
        assert_eq!(vec![vec![0x01],], state.list());

        l.apply(&mut state.boxed()).unwrap();
        assert_eq!(vec![vec![0x01],], state.list());

        assert_eq!(Ok(None), l.apply(&mut state.boxed()));
        assert_eq!(vec![vec![0x01],], state.list());

        // The last commit entry won't be persisted
        let l = Log::new(store).unwrap();
        assert_eq!((3, 2), l.get_last());
        assert_eq!((2, 2), l.get_committed());
        assert_eq!((2, 2), l.get_applied());
        assert_eq!(vec![vec![0x01],], state.list());
    }

    #[test]
    fn apply_partial_commit_and_recover() {
        let (mut l, store) = setup();
        setup_appends(&mut l);
        l.commit(3).unwrap();

        let state = TestState::new();
        l.apply(&mut state.boxed()).unwrap();
        l.apply(&mut state.boxed()).unwrap();
        assert_eq!(vec![vec![0x01],], state.list());

        // The last commit entry won't be persisted
        let mut l = Log::new(store).unwrap();
        assert_eq!((3, 2), l.get_last());
        assert_eq!((2, 2), l.get_committed());
        assert_eq!((2, 2), l.get_applied());

        // after recovering, finalizes the commits
        l.commit(3).unwrap();
        l.apply(&mut state.boxed()).unwrap();
        assert_eq!((3, 2), l.get_last());
        assert_eq!((3, 2), l.get_committed());
        assert_eq!((3, 2), l.get_applied());
        assert_eq!(vec![vec![0x01], vec![0x03]], state.list());
    }

    #[test]
    fn has() {
        let (mut l, _) = setup();
        l.append(Entry {
            term: 2,
            command: Some(vec![0x01]),
        })
        .unwrap();

        assert_eq!(true, l.has(1, 2).unwrap());
        assert_eq!(true, l.has(0, 0).unwrap());
        assert_eq!(false, l.has(0, 1).unwrap());
        assert_eq!(false, l.has(1, 0).unwrap());
        assert_eq!(false, l.has(1, 3).unwrap());
        assert_eq!(false, l.has(2, 0).unwrap());
        assert_eq!(false, l.has(2, 1).unwrap());
    }
}
