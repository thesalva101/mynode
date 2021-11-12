use super::schema;
use crate::serializer::serialize;
use crate::store::Store;
use crate::Error;
use crate::{serializer::deserialize, store};
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct Storage {
    kv: Arc<RwLock<Box<dyn Store>>>,
}

impl std::fmt::Debug for Storage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Storage")
    }
}

impl Storage {
    /// Creates a new Storage
    pub fn new<S: Store>(store: S) -> Self {
        Storage {
            kv: Arc::new(RwLock::new(Box::new(store))),
        }
    }

    /// Checks if a table exists
    pub fn table_exists(&self, table_name: &str) -> Result<bool, Error> {
        let table_key = Self::key_table(table_name);
        let table = self.kv.read()?.get(&table_key)?;
        Ok(table.is_some())
    }

    /// Fetches a table schema
    pub fn get_table(&self, table_name: &str) -> Result<schema::Table, Error> {
        let table_key = Self::key_table(table_name);
        let table = self
            .kv
            .read()?
            .get(&table_key)?
            .ok_or(Error::Value(format!("Table {} does not exist", table_name)))?;
        deserialize(table)
    }

    /// Creates a table
    pub fn create_table(&mut self, table: schema::Table) -> Result<(), Error> {
        if self.table_exists(&table.name)? {
            Err(Error::Value(format!("Table {} already exists", table.name)))
        } else {
            let table_name = Self::key_table(&table.name);
            let serialized_table = serialize(table)?;
            self.kv.write()?.set(&table_name, serialized_table)
        }
    }

    /// Deletes a table
    pub fn drop_table(&mut self, table_name: &str) -> Result<(), Error> {
        let table_key = Self::key_table(table_name);
        self.kv.write()?.delete(&table_key)
    }

    /// Generates a key for a table
    fn key_table(table: &str) -> String {
        format!("schema.table.{}", table)
    }
}
