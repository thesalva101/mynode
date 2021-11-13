use super::super::types::Row;
use super::{Context, Node};
use crate::sql::expression::Expressions;
use crate::Error;

/// A projection node
#[derive(Debug)]
pub struct Projection {
    source: Box<dyn Node>,
    labels: Vec<String>,
    expressions: Expressions,
}

impl Projection {
    pub fn new(source: Box<dyn Node>, labels: Vec<String>, expressions: Expressions) -> Self {
        Self {
            source,
            labels,
            expressions,
        }
    }
}

impl Node for Projection {
    fn execute(&mut self, ctx: &mut Context) -> Result<(), Error> {
        self.source.execute(ctx)
    }
}

impl Iterator for Projection {
    type Item = Result<Row, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.source.next()? {
            Err(err) => Some(Err(err)),
            _ => Some(self.expressions.iter().map(|e| e.evaluate()).collect()),
        }
    }
}
