use super::super::schema;
use super::super::storage::Storage;
use super::super::types::Row;
use super::expression::{Expression, Expressions};
use crate::Error;

/// A plan node
#[derive(Debug)]
pub enum Node {
    DDL {
        storage: Box<Storage>,
        ddl: DDL,
    },
    DML {
        storage: Box<Storage>,
        dml: DML,
    },
    Projection {
        labels: Vec<String>,
        source: Box<Node>,
        expressions: Vec<Expression>,
    },
    Nothing {
        done: bool,
    },
}

impl Iterator for Node {
    type Item = Result<Row, Error>;

    fn next(&mut self) -> Option<Result<Row, Error>> {
        match self {
            Node::DDL { storage, ddl } => {
                ddl.execute(*storage.clone()).unwrap();
                None
            }
            Node::DML { storage, dml } => {
                dml.execute(*storage.clone()).unwrap();
                None
            }
            Node::Projection {
                source,
                expressions,
                ..
            } => {
                if let Err(err) = source.next()? {
                    Some(Err(err))
                } else {
                    Some(expressions.iter().map(|e| e.evaluate()).collect())
                }
            }
            Node::Nothing { ref mut done } => {
                if !*done {
                    *done = true;
                    Some(Ok(vec![]))
                } else {
                    None
                }
            }
        }
    }
}

/// A DDL operation
#[derive(Debug)]
pub enum DDL {
    CreateTable(schema::Table),
    DropTable(String),
}

impl DDL {
    fn execute(&self, mut storage: Storage) -> Result<(), Error> {
        match self {
            Self::CreateTable(schema) => storage.create_table(schema.clone()),
            Self::DropTable(name) => storage.drop_table(name),
        }
    }
}

/// A DML operation
#[derive(Debug)]
pub enum DML {
    Insert(String, Vec<Expressions>),
}

impl DML {
    fn execute(&self, mut storage: Storage) -> Result<(), Error> {
        match self {
            Self::Insert(table, values) => {
                for exprs in values {
                    let mut row = Row::new();
                    for expr in exprs {
                        row.push(expr.evaluate()?);
                    }
                    storage.create_row(table, row)?;
                }
                Ok(())
            }
        }
    }
}
