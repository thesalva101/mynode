mod create_table;
mod drop_table;
mod insert;
mod nothing;
mod projection;
mod scan;

use self::nothing::Nothing;
use self::projection::Projection;
use self::scan::Scan;
use super::ast::{self, ColumnSpec, Statement};
use super::expression::Expression;
use super::schema::{Column, Table};
use super::storage::Storage;
use super::types::{Row, Value};
use crate::Error;
use create_table::CreateTable;
use drop_table::DropTable;
use insert::Insert;

/// A plan
#[derive(Debug)]
pub struct Plan {
    /// The plan root
    pub root: Box<dyn Node>,
}

impl Plan {
    pub fn build(statement: Statement) -> Result<Self, Error> {
        Planner::new().build(statement)
    }

    pub fn execute(mut self, mut context: Context) -> Result<ResultSet, Error> {
        self.root.execute(&mut context)?;
        Ok(ResultSet { root: self.root })
    }
}

/// A plan execution context
pub struct Context {
    /// The underlying storage
    pub storage: Box<Storage>,
}

/// A plan execution result
pub struct ResultSet {
    root: Box<dyn Node>,
}

impl Iterator for ResultSet {
    type Item = Result<Row, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.root.next()
    }
}

/// A plan node
pub trait Node:
    Iterator<Item = Result<Row, Error>> + std::fmt::Debug + Send + Sync + 'static
{
    /// Execute starts execution of the plan
    fn execute(&mut self, ctx: &mut Context) -> Result<(), Error>;
}

impl<N: Node> From<N> for Box<dyn Node> {
    fn from(n: N) -> Self {
        Box::new(n)
    }
}
/// The plan builder
struct Planner {}

impl Planner {
    /// Creates a new planner
    pub fn new() -> Self {
        Self {}
    }

    /// Builds a plan tree for an AST statement
    pub fn build(&self, statement: Statement) -> Result<Plan, Error> {
        Ok(Plan {
            root: self.build_statement(statement)?,
        })
    }

    /// Builds a plan node for a statement
    fn build_statement(&self, statement: Statement) -> Result<Box<dyn Node>, Error> {
        Ok(match statement {
            Statement::CreateTable { name, columns } => {
                CreateTable::new(self.build_schema_table(name, columns)?).into()
            }
            Statement::DropTable(name) => DropTable::new(name).into(),
            Statement::Insert { table, values, .. } => {
                // FIXME Needs to handle columns
                Insert::new(
                    table,
                    values
                        .into_iter()
                        .map(|exprs| exprs.into_iter().map(|expr| expr.into()).collect())
                        .collect(),
                )
                .into()
            }
            Statement::Select { select, from } => {
                let mut n: Box<dyn Node> = match from {
                    // FIXME Handle multiple FROM tables
                    Some(from) => Scan::new(from.tables[0].clone()).into(),
                    None if select.expressions.is_empty() => {
                        return Err(Error::Value("Can't select * without a table".into()))
                    }
                    None => Nothing::new().into(),
                };
                if !select.expressions.is_empty() {
                    n = Projection::new(
                        n,
                        select
                            .labels
                            .into_iter()
                            .map(|l| l.unwrap_or_else(|| "?".into()))
                            .collect(),
                        self.build_expressions(select.expressions)?,
                    )
                    .into();
                };
                n
            }
        })
    }

    /// Builds a plan expression from an AST expression
    fn build_expression(&self, expr: ast::Expression) -> Result<Expression, Error> {
        Ok(expr.into())
    }

    /// Builds an array of plan expressions from AST expressions
    fn build_expressions(&self, exprs: Vec<ast::Expression>) -> Result<Vec<Expression>, Error> {
        exprs
            .into_iter()
            .map(|e| self.build_expression(e))
            .collect()
    }

    /// Builds a table schema from an AST CreateTable node
    fn build_schema_table(
        &self,
        name: String,
        columnspecs: Vec<ColumnSpec>,
    ) -> Result<Table, Error> {
        let primary_keys: Vec<&ColumnSpec> = columnspecs.iter().filter(|c| c.primary_key).collect();
        if primary_keys.is_empty() {
            return Err(Error::Value(format!(
                "No primary key defined for table {}",
                name
            )));
        } else if primary_keys.len() > 1 {
            return Err(Error::Value(format!(
                "{} primary keys defined for table {}, must set exactly 1",
                primary_keys.len(),
                name
            )));
        }
        let primary_key = primary_keys[0];
        if let Some(true) = primary_key.nullable {
            return Err(Error::Value("Primary key cannot be nullable".into()));
        }

        Ok(Table {
            name,
            primary_key: primary_key.name.clone(),
            columns: columnspecs
                .into_iter()
                .map(|spec| Column {
                    name: spec.name,
                    datatype: spec.datatype,
                    nullable: spec.nullable.unwrap_or(!spec.primary_key),
                })
                .collect(),
        })
    }
}

/// Helpers to convert AST expressions into plan expressions
impl From<ast::Expression> for Expression {
    fn from(expr: ast::Expression) -> Self {
        match expr {
            ast::Expression::Literal(l) => Expression::Constant(l.into()),
            ast::Expression::Operation(op) => match op {
                // Logical operators
                ast::Operation::And(lhs, rhs) => Self::And(lhs.into(), rhs.into()),
                ast::Operation::Not(expr) => Self::Not(expr.into()),
                ast::Operation::Or(lhs, rhs) => Self::Or(lhs.into(), rhs.into()),

                // Comparison operators
                ast::Operation::CompareEQ(lhs, rhs) => Self::CompareEQ(lhs.into(), rhs.into()),
                ast::Operation::CompareGT(lhs, rhs) => Self::CompareGT(lhs.into(), rhs.into()),
                ast::Operation::CompareGTE(lhs, rhs) => Self::CompareGTE(lhs.into(), rhs.into()),
                ast::Operation::CompareLT(lhs, rhs) => Self::CompareLT(lhs.into(), rhs.into()),
                ast::Operation::CompareLTE(lhs, rhs) => Self::CompareLTE(lhs.into(), rhs.into()),
                ast::Operation::CompareNE(lhs, rhs) => Self::CompareNE(lhs.into(), rhs.into()),

                // Mathematical operators
                ast::Operation::Add(lhs, rhs) => Self::Add(lhs.into(), rhs.into()),
                ast::Operation::Divide(lhs, rhs) => Self::Divide(lhs.into(), rhs.into()),
                ast::Operation::Exponentiate(lhs, rhs) => {
                    Self::Exponentiate(lhs.into(), rhs.into())
                }
                ast::Operation::Factorial(expr) => Self::Factorial(expr.into()),
                ast::Operation::Modulo(lhs, rhs) => Self::Modulo(lhs.into(), rhs.into()),
                ast::Operation::Multiply(lhs, rhs) => Self::Multiply(lhs.into(), rhs.into()),
                ast::Operation::Negate(expr) => Self::Negate(expr.into()),
                ast::Operation::Subtract(lhs, rhs) => Self::Subtract(lhs.into(), rhs.into()),
            },
        }
    }
}

impl From<Box<ast::Expression>> for Box<Expression> {
    fn from(expr: Box<ast::Expression>) -> Self {
        Box::new((*expr).into())
    }
}

impl From<ast::Literal> for Value {
    fn from(literal: ast::Literal) -> Self {
        match literal {
            ast::Literal::Null => Value::Null,
            ast::Literal::Boolean(b) => b.into(),
            ast::Literal::Float(f) => f.into(),
            ast::Literal::Integer(i) => i.into(),
            ast::Literal::String(s) => s.into(),
        }
    }
}
