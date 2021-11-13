use super::{Iter, KVPair, Store};
use crate::Error;
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

/// A Store is a persistent key-value store for values of type V, serialized as
/// MessagePack. It's currently implemented as a transient in-memory store while
/// prototyping the interface.
#[derive(Clone, Debug)]
pub struct KVMemory {
    data: Arc<RwLock<BTreeMap<String, Vec<u8>>>>,
}

impl KVMemory {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }
}

impl Store for KVMemory {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        Ok(self.data.clone().read()?.get(key).cloned())
    }

    fn set(&mut self, key: &str, value: Vec<u8>) -> Result<(), Error> {
        self.data.clone().write()?.insert(key.to_string(), value);
        Ok(())
    }

    fn delete(&mut self, key: &str) -> Result<(), Error> {
        self.data.clone().write()?.remove(key);
        Ok(())
    }

    fn iter_prefix(&self, prefix: &str) -> Box<dyn Iterator<Item = Result<KVPair, Error>>> {
        let from = prefix.to_string();
        let to = from.clone() + &std::char::MAX.to_string();
        info!("from {} to {}", from, to);
        Box::new(Iter::from(self.data.read().unwrap().range(from..to)))
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::Suite;
    use super::*;

    #[test]
    fn suite() {
        Suite::new(|| Box::new(KVMemory::new())).test()
    }
}
