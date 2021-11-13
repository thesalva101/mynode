mod file;
mod kvmemory;
mod raft;

use crate::Error;
pub use file::File;
pub use kvmemory::KVMemory;
pub use raft::Raft;

type KVPair = (String, Vec<u8>);
type Range = dyn Iterator<Item = Result<KVPair, Error>> + Sync + Send;

pub trait Store: 'static + Sync + Send + std::fmt::Debug {
    fn delete(&mut self, key: &str) -> Result<(), Error>;
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, Error>;
    fn set(&mut self, key: &str, value: Vec<u8>) -> Result<(), Error>;

    /// Returns an iterator over all pairs in the store under a key prefix
    fn iter_prefix(&self, prefix: &str) -> Box<Range>;
}

/// This is a terrible, temporary iterator implementation which is prepopulated
/// with all data, to avoid having to deal with trait lifetimes right now.
struct Iter {
    stack: Vec<Result<KVPair, Error>>,
}

impl Iter {
    fn from<'a, I>(iter: I) -> Self
    where
        I: DoubleEndedIterator + Iterator<Item = (&'a String, &'a Vec<u8>)>,
    {
        Self {
            stack: iter
                .map(|(k, v)| Ok((k.clone(), v.clone())))
                .rev()
                .collect(),
        }
    }

    fn from_vec(vec: Vec<KVPair>) -> Self {
        Self {
            stack: vec.into_iter().map(Ok).rev().collect(),
        }
    }
}

impl Iterator for Iter {
    type Item = Result<KVPair, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.stack.pop()
    }
}

pub fn get_obj<'de, V: serde::Deserialize<'de>>(
    store: &dyn Store,
    key: &str,
) -> Result<Option<V>, Error> {
    Ok(match store.get(key)? {
        Some(v) => {
            let mut deserializer = rmps::Deserializer::new(&v[..]);
            Some(serde::Deserialize::deserialize(&mut deserializer)?)
        }
        None => None,
    })
}

pub fn set_obj<V: serde::Serialize>(
    store: &mut dyn Store,
    key: &str,
    value: V,
) -> Result<(), Error> {
    let mut buffer = Vec::new();
    value.serialize(&mut rmps::Serializer::new(&mut buffer))?;
    store.set(key, buffer)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub struct Suite {
        factory: Box<dyn Fn() -> Box<dyn Store>>,
    }

    impl Suite {
        pub fn new<F>(setup: F) -> Self
        where
            F: 'static + Fn() -> Box<dyn Store>,
        {
            Self {
                factory: Box::new(setup),
            }
        }

        fn setup(&self) -> Box<dyn Store> {
            (&self.factory)()
        }

        pub fn test(&self) {
            self.test_delete();
            self.test_get();
            self.test_iter_prefix();
            self.test_set();
        }

        pub fn test_delete(&self) {
            let mut s = self.setup();
            s.set("a", vec![0x01]).unwrap();
            assert_eq!(vec![0x01], s.get("a").unwrap().unwrap());
            s.delete("a").unwrap();
            assert_eq!(None, s.get("a").unwrap());
            s.delete("b").unwrap();
        }

        pub fn test_get(&self) {
            let mut s = self.setup();
            s.set("a", vec![0x01]).unwrap();
            assert_eq!(vec![0x01], s.get("a").unwrap().unwrap());
            assert_eq!(None, s.get("b").unwrap());
        }

        pub fn test_set(&self) {
            let mut s = self.setup();
            s.set("a", vec![0x01]).unwrap();
            assert_eq!(vec![0x01], s.get("a").unwrap().unwrap());
            s.set("a", vec![0x02]).unwrap();
            assert_eq!(vec![0x02], s.get("a").unwrap().unwrap());
        }

        pub fn test_rmps() {
            let mut store = KVMemory::new();
            set_obj(&mut store, "x", String::from("xis")).unwrap();
            set_obj(&mut store, "y", String::from("uai")).unwrap();
            assert_eq!(get_obj::<String>(&store, "x").unwrap().unwrap(), "xis");
            assert_eq!(get_obj::<String>(&store, "y").unwrap().unwrap(), "uai");
            store.delete("x").unwrap();
            assert_eq!(get_obj::<String>(&store, "x").unwrap(), Option::None);
        }

        pub fn test_iter_prefix(&self) {
            let mut s = self.setup();
            s.set("a", vec![0x01]).unwrap();
            s.set("b", vec![0x02]).unwrap();
            s.set("ba", vec![0x02, 0x01]).unwrap();
            s.set("bb", vec![0x02, 0x02]).unwrap();
            s.set("c", vec![0x03]).unwrap();

            assert_eq!(
                vec![
                    ("b".to_string(), vec![0x02]),
                    ("ba".to_string(), vec![0x02, 0x01]),
                    ("bb".to_string(), vec![0x02, 0x02]),
                ],
                s.iter_prefix("b")
                    .collect::<Result<Vec<(String, Vec<u8>)>, Error>>()
                    .unwrap()
            )
        }
    }
}
