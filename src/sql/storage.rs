use super::schema;
use super::schema::Table;
use super::types;
use crate::serializer::deserialize;
use crate::serializer::serialize;
use crate::store::Store;
use crate::Error;
use std::sync::{Arc, RwLock};

const TABLE_PREFIX: &str = "schema.table";

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

    /// List all the existing tables
    pub fn list_tables(&self) -> Result<Vec<String>, Error> {
        let mut iter = self.kv.read()?.iter_prefix(TABLE_PREFIX);
        let mut tables = Vec::new();
        while let Some((_, value)) = iter.next().transpose()? {
            let schema: schema::Table = deserialize(value)?;
            tables.push(schema.name)
        }
        Ok(tables)
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

    /// Creates a row in a table
    pub fn create_row(&mut self, table_name: &str, row: types::Row) -> Result<(), Error> {
        let table = self.get_table(&table_name)?;
        let id = row
            .get(table.get_primary_key_index())
            .ok_or_else(|| Error::Value("No primary key value".into()))?;
        let row_key = Self::key_row(table_name, &id.to_string());
        self.kv.write()?.set(&row_key, serialize(row)?)
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
        format!("{}.{}", TABLE_PREFIX, table)
    }

    /// Generates a key for a row
    fn key_row(table: &str, id: &str) -> String {
        format!("{}.{}", Self::key_table(table), id)
    }
}
