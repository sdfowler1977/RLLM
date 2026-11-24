//! `executed_count` from tools/package.py.
//! A count is a Python `int` that is not a `bool` and is greater than zero.
//! `passed` as a list contributes its length. A later `count` comparison uses
//! Python equality, so `True` matches 1 and an integral float matches that integer.

#[derive(Debug)]
pub enum FieldValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    List(Vec<FieldValue>),
    Object(Vec<(String, FieldValue)>),
}

#[derive(Debug, PartialEq, Eq)]
pub enum CountFault {
    Assertion,
    MissingCount,
    TypeError,
}

fn field<'a>(fields: &'a [(&str, FieldValue)], name: &str) -> Option<&'a FieldValue> {
    fields.iter().rev().find(|(key, _)| *key == name).map(|(_, value)| value)
}

fn strict_int(value: &FieldValue) -> Result<i64, CountFault> {
    match value {
        FieldValue::Int(number) => Ok(*number),
        _ => Err(CountFault::Assertion),
    }
}

fn python_len(value: &FieldValue) -> Result<i64, CountFault> {
    let length = match value {
        FieldValue::List(items) => items.len(),
        FieldValue::Object(pairs) => pairs.len(),
        FieldValue::Text(text) => text.chars().count(),
        _ => return Err(CountFault::TypeError),
    };
    i64::try_from(length).map_err(|_| CountFault::TypeError)
}

fn int_eq_float(count: i64, value: f64) -> bool {
    if !value.is_finite() {
        return false;
    }
    let lowest = i64::MIN as f64;
    let one_past_i64 = 9223372036854775808.0_f64;
    if value < lowest || value >= one_past_i64 || value.fract() != 0.0 {
        return false;
    }
    (value as i64) == count && (count as f64) == value
}

fn python_equal_int(count: i64, other: &FieldValue) -> bool {
    match other {
        FieldValue::Int(number) => count == *number,
        FieldValue::Bool(value) => count == i64::from(*value),
        FieldValue::Float(value) => int_eq_float(count, *value),
        _ => false,
    }
}

pub fn executed_count(fields: &[(&str, FieldValue)]) -> Result<i64, CountFault> {
    let count = if let Some(passed) = field(fields, "passed") {
        if let FieldValue::List(items) = passed {
            i64::try_from(items.len()).map_err(|_| CountFault::Assertion)?
        } else {
            strict_int(passed)?
        }
    } else {
        match field(fields, "count") {
            Some(value) => strict_int(value)?,
            None => return Err(CountFault::MissingCount),
        }
    };
    if count <= 0 {
        return Err(CountFault::Assertion);
    }
    if let Some(checks) = field(fields, "checks") {
        if count != python_len(checks)? {
            return Err(CountFault::Assertion);
        }
    }
    if let Some(declared) = field(fields, "count") {
        if !python_equal_int(count, declared) {
            return Err(CountFault::Assertion);
        }
    }
    Ok(count)
}
