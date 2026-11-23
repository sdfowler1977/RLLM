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

#[cfg(test)]
fn texts(items: &[&str]) -> FieldValue {
    FieldValue::List(items.iter().copied().map(|item| FieldValue::Text(item.to_string())).collect())
}

#[cfg(test)]
fn ints(items: &[i64]) -> FieldValue {
    FieldValue::List(items.iter().copied().map(FieldValue::Int).collect())
}

#[test]
fn passed_list_of_two() {
    // {'passed': ['a', 'b']} -> 2
    assert_eq!(executed_count(&[("passed", texts(&["a", "b"]))]), Ok(2));
}

#[test]
fn passed_integer_five() {
    // {'passed': 5} -> 5
    assert_eq!(executed_count(&[("passed", FieldValue::Int(5))]), Ok(5));
}

#[test]
fn count_field_four() {
    // {'count': 4} -> 4
    assert_eq!(executed_count(&[("count", FieldValue::Int(4))]), Ok(4));
}

#[test]
fn passed_and_count_agree() {
    // {'passed': 4, 'count': 4} -> 4
    assert_eq!(
        executed_count(&[("passed", FieldValue::Int(4)), ("count", FieldValue::Int(4))]),
        Ok(4)
    );
}

#[test]
fn checks_array_length() {
    // {'passed': 1, 'checks': ['one']} -> 1
    assert_eq!(
        executed_count(&[("passed", FieldValue::Int(1)), ("checks", texts(&["one"]))]),
        Ok(1)
    );
}

#[test]
fn list_checks_and_count_agree() {
    // {'passed': ['a', 'b'], 'checks': ['x', 'y'], 'count': 2} -> 2
    assert_eq!(
        executed_count(&[
            ("passed", texts(&["a", "b"])),
            ("checks", texts(&["x", "y"])),
            ("count", FieldValue::Int(2)),
        ]),
        Ok(2)
    );
}

#[test]
fn count_true_equals_length_one() {
    // {'passed': ['a'], 'count': True} -> 1
    assert_eq!(
        executed_count(&[("passed", texts(&["a"])), ("count", FieldValue::Bool(true))]),
        Ok(1)
    );
}

#[test]
fn count_float_one_equals_length_one() {
    // {'passed': [1], 'count': 1.0} -> 1
    assert_eq!(
        executed_count(&[("passed", ints(&[1])), ("count", FieldValue::Float(1.0))]),
        Ok(1)
    );
}

#[test]
fn passed_one_equals_float_one() {
    // {'passed': 1, 'count': 1.0} -> 1
    assert_eq!(
        executed_count(&[("passed", FieldValue::Int(1)), ("count", FieldValue::Float(1.0))]),
        Ok(1)
    );
}

#[test]
fn exact_two_to_the_53_matches_its_float() {
    // {'passed': 9007199254740992, 'count': float(9007199254740992)} -> 9007199254740992
    assert_eq!(
        executed_count(&[(
            "passed",
            FieldValue::Int(9007199254740992),
        ), (
            "count",
            FieldValue::Float(9007199254740992.0),
        )]),
        Ok(9007199254740992)
    );
}

#[test]
fn checks_string_length() {
    // {'passed': [1, 2], 'checks': 'ab'} -> 2
    assert_eq!(
        executed_count(&[
            ("passed", ints(&[1, 2])),
            ("checks", FieldValue::Text("ab".to_string())),
        ]),
        Ok(2)
    );
}

#[test]
fn checks_object_length() {
    // {'passed': [1], 'checks': {'k': 1}} -> 1
    let checks = FieldValue::Object(vec![("k".to_string(), FieldValue::Int(1))]);
    assert_eq!(executed_count(&[("passed", ints(&[1])), ("checks", checks)]), Ok(1));
}

#[test]
fn emoji_checks_length_one() {
    // {'passed': ['a'], 'checks': '😀'} -> 1
    assert_eq!(
        executed_count(&[
            ("passed", texts(&["a"])),
            ("checks", FieldValue::Text("😀".to_string())),
        ]),
        Ok(1)
    );
}

#[test]
fn combining_character_checks_length_two() {
    // {'passed': ['a', 'b'], 'checks': 'e\u0301'} -> 2
    assert_eq!(
        executed_count(&[
            ("passed", texts(&["a", "b"])),
            ("checks", FieldValue::Text("e\u{0301}".to_string())),
        ]),
        Ok(2)
    );
}

#[test]
fn count_float_two_point_five_rejected() {
    // {'passed': [1, 2], 'count': 2.5} -> AssertionError
    assert_eq!(
        executed_count(&[("passed", ints(&[1, 2])), ("count", FieldValue::Float(2.5))]),
        Err(CountFault::Assertion)
    );
}

#[test]
fn count_true_does_not_equal_length_two() {
    // {'passed': ['a', 'b'], 'count': True} -> AssertionError
    assert_eq!(
        executed_count(&[("passed", texts(&["a", "b"])), ("count", FieldValue::Bool(true))]),
        Err(CountFault::Assertion)
    );
}

#[test]
fn large_integer_not_equal_to_rounded_float() {
    // {'passed': 2**53+1, 'count': float(2**53)} -> AssertionError
    assert_eq!(
        executed_count(&[(
            "passed",
            FieldValue::Int(9007199254740993),
        ), (
            "count",
            FieldValue::Float(9007199254740992.0),
        )]),
        Err(CountFault::Assertion)
    );
}

#[test]
fn i64_max_does_not_equal_two_to_the_63() {
    // {'passed': 2**63-1, 'count': float(2**63)} -> AssertionError
    assert_eq!(
        executed_count(&[(
            "passed",
            FieldValue::Int(9223372036854775807),
        ), (
            "count",
            FieldValue::Float(9223372036854775808.0),
        )]),
        Err(CountFault::Assertion)
    );
}

#[test]
fn passed_true_rejected() {
    // {'passed': True} -> AssertionError
    assert_eq!(
        executed_count(&[("passed", FieldValue::Bool(true))]),
        Err(CountFault::Assertion)
    );
}

#[test]
fn passed_false_rejected() {
    // {'passed': False} -> AssertionError
    assert_eq!(
        executed_count(&[("passed", FieldValue::Bool(false))]),
        Err(CountFault::Assertion)
    );
}

#[test]
fn passed_zero_rejected() {
    // {'passed': 0} -> AssertionError
    assert_eq!(executed_count(&[("passed", FieldValue::Int(0))]), Err(CountFault::Assertion));
}

#[test]
fn passed_empty_list_rejected() {
    // {'passed': []} -> AssertionError
    assert_eq!(executed_count(&[("passed", ints(&[]))]), Err(CountFault::Assertion));
}

#[test]
fn passed_negative_rejected() {
    // {'passed': -4} -> AssertionError
    assert_eq!(executed_count(&[("passed", FieldValue::Int(-4))]), Err(CountFault::Assertion));
}

#[test]
fn passed_and_count_differ() {
    // {'passed': 3, 'count': 4} -> AssertionError
    assert_eq!(
        executed_count(&[("passed", FieldValue::Int(3)), ("count", FieldValue::Int(4))]),
        Err(CountFault::Assertion)
    );
}

#[test]
fn checks_shorter_than_passed() {
    // {'passed': 3, 'checks': ['only']} -> AssertionError
    assert_eq!(
        executed_count(&[("passed", FieldValue::Int(3)), ("checks", texts(&["only"]))]),
        Err(CountFault::Assertion)
    );
}

#[test]
fn passed_string_rejected() {
    // {'passed': '4'} -> AssertionError
    assert_eq!(
        executed_count(&[("passed", FieldValue::Text("4".to_string()))]),
        Err(CountFault::Assertion)
    );
}

#[test]
fn passed_float_rejected() {
    // {'passed': 1.5} -> AssertionError
    assert_eq!(
        executed_count(&[("passed", FieldValue::Float(1.5))]),
        Err(CountFault::Assertion)
    );
}

#[test]
fn count_bool_source_rejected() {
    // {'count': True} -> AssertionError
    assert_eq!(
        executed_count(&[("count", FieldValue::Bool(true))]),
        Err(CountFault::Assertion)
    );
}

#[test]
fn count_float_source_rejected() {
    // {'count': 2.0} -> AssertionError
    assert_eq!(
        executed_count(&[("count", FieldValue::Float(2.0))]),
        Err(CountFault::Assertion)
    );
}

#[test]
fn passed_null_rejected() {
    // {'passed': None} -> AssertionError
    assert_eq!(executed_count(&[("passed", FieldValue::Null)]), Err(CountFault::Assertion));
}

#[test]
fn missing_count_key() {
    // {'scope': 'only'} -> KeyError
    assert_eq!(
        executed_count(&[("scope", FieldValue::Text("only".to_string()))]),
        Err(CountFault::MissingCount)
    );
}

#[test]
fn checks_integer_is_type_error() {
    // {'passed': [1, 2], 'checks': 2} -> TypeError
    assert_eq!(
        executed_count(&[("passed", ints(&[1, 2])), ("checks", FieldValue::Int(2))]),
        Err(CountFault::TypeError)
    );
}

#[test]
fn checks_null_is_type_error() {
    // {'passed': [1, 2], 'checks': None} -> TypeError
    assert_eq!(
        executed_count(&[("passed", ints(&[1, 2])), ("checks", FieldValue::Null)]),
        Err(CountFault::TypeError)
    );
}
