use super::super::types::Value;
use crate::Error;

/// An expression
pub enum Expression {
    Constant(Value),
    Add(Box<Expression>, Box<Expression>),
    Divide(Box<Expression>, Box<Expression>),
    Exponentiate(Box<Expression>, Box<Expression>),
    Factorial(Box<Expression>),
    Modulo(Box<Expression>, Box<Expression>),
    Multiply(Box<Expression>, Box<Expression>),
    Negate(Box<Expression>),
    Subtract(Box<Expression>, Box<Expression>),
}

impl Expression {
    /// Evaluates an expression to a value
    pub fn evaluate(&self) -> Result<Value, Error> {
        use Value::*;
        Ok(match self {
            Expression::Add(lhs, rhs) => match (lhs.evaluate()?, rhs.evaluate()?) {
                (Integer(lhs), Integer(rhs)) => Integer(lhs + rhs),
                (Integer(lhs), Float(rhs)) => Float(lhs as f64 + rhs),
                (Float(lhs), Integer(rhs)) => Float(lhs + rhs as f64),
                (Float(lhs), Float(rhs)) => Float(lhs + rhs),
                (lhs, rhs) => return Err(Error::Value(format!("Can't add {} and {}", lhs, rhs))),
            },
            Expression::Divide(lhs, rhs) => match (lhs.evaluate()?, rhs.evaluate()?) {
                (Integer(lhs), Integer(rhs)) => Integer(lhs / rhs),
                (Integer(lhs), Float(rhs)) => Float(lhs as f64 / rhs),
                (Float(lhs), Integer(rhs)) => Float(lhs / rhs as f64),
                (Float(lhs), Float(rhs)) => Float(lhs / rhs),
                (lhs, rhs) => {
                    return Err(Error::Value(format!("Can't divide {} and {}", lhs, rhs)))
                }
            },
            Expression::Exponentiate(lhs, rhs) => match (lhs.evaluate()?, rhs.evaluate()?) {
                // FIXME Handle overflow
                (Integer(lhs), Integer(rhs)) => Integer(lhs.pow(rhs as u32)),
                (Integer(lhs), Float(rhs)) => Float((lhs as f64).powi(rhs as i32)),
                (Float(lhs), Integer(rhs)) => Float((lhs).powi(rhs as i32)),
                (Float(lhs), Float(rhs)) => Float((lhs).powf(rhs)),
                (lhs, rhs) => {
                    return Err(Error::Value(format!(
                        "Can't exponentiate {} and {}",
                        lhs, rhs
                    )))
                }
            },
            Expression::Factorial(v) => match v.evaluate()? {
                Integer(v) => Integer((1..=v).fold(1, |a, b| a * b as i64)),
                v => return Err(Error::Value(format!("Can't take factorial of {}", v))),
            },
            Expression::Modulo(lhs, rhs) => match (lhs.evaluate()?, rhs.evaluate()?) {
                // The % operator in Rust is remainder, not modulo, so we have to do a bit of
                // acrobatics to make it work right
                (Integer(lhs), Integer(rhs)) => Integer(((lhs % rhs) + rhs) % rhs),
                (Integer(lhs), Float(rhs)) => Float(((lhs as f64 % rhs) + rhs) % rhs),
                (Float(lhs), Integer(rhs)) => Float(((lhs % rhs as f64) + rhs as f64) % rhs as f64),
                (Float(lhs), Float(rhs)) => Float(((lhs % rhs) + rhs) % rhs),
                (lhs, rhs) => {
                    return Err(Error::Value(format!(
                        "Can't take modulo of {} and {}",
                        lhs, rhs
                    )))
                }
            },
            Expression::Multiply(lhs, rhs) => match (lhs.evaluate()?, rhs.evaluate()?) {
                (Integer(lhs), Integer(rhs)) => Integer(lhs * rhs),
                (Integer(lhs), Float(rhs)) => Float(lhs as f64 * rhs),
                (Float(lhs), Integer(rhs)) => Float(lhs * rhs as f64),
                (Float(lhs), Float(rhs)) => Float(lhs * rhs),
                (lhs, rhs) => {
                    return Err(Error::Value(format!("Can't multiply {} and {}", lhs, rhs)))
                }
            },
            Expression::Negate(expr) => match expr.evaluate()? {
                Integer(v) => Integer(-v),
                Float(v) => Float(-v),
                v => return Err(Error::Value(format!("Can't negate {}", v))),
            },
            Expression::Subtract(lhs, rhs) => match (lhs.evaluate()?, rhs.evaluate()?) {
                (Integer(lhs), Integer(rhs)) => Integer(lhs - rhs),
                (Integer(lhs), Float(rhs)) => Float(lhs as f64 - rhs),
                (Float(lhs), Integer(rhs)) => Float(lhs - rhs as f64),
                (Float(lhs), Float(rhs)) => Float(lhs - rhs),
                (lhs, rhs) => {
                    return Err(Error::Value(format!("Can't subtract {} and {}", lhs, rhs)))
                }
            },

            Expression::Constant(v) => v.clone(),
        })
    }
}
