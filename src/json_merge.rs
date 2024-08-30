// Copyright 2024 Aurelia Peters
//
// This file is part of GraphQL Cacher.
// 
// GraphQL Cacher is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
// 
// GraphQL Cacher is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
// 
// You should have received a copy of the GNU General Public License along with GraphQL Cacher. If not, see <https://www.gnu.org/licenses/>. 
use itertools::Itertools;
use serde_json::Value;
// use tracing::{debug, error, warn};
use tracing::error;
pub trait Merge {
    /// Method use to merge two Json Values : ValueA <- ValueB.
    fn merge(&mut self, new_json_value: &Value);
}

impl Merge for serde_json::Value {
    fn merge(&mut self, new_json_value: &Value) {
        merge(self, new_json_value);
    }
}

/// Merge the JSON results of two GraphQL queries.
/// If, for a given path, the two query results each contain an array of objects,
/// each object in the array in query B will be merged with its counterpart in query A.
/// Example:
///   Query result A:
///   { "data": { "foo": [ { "name": "alpha" }, { "name": "beta" } ] } }
///   Query result B:
///   { "data": { "foo": [ { "color": "red" }, { "color": "green" } ] } }
///   Will produce:
///   { "data": { "foo": [ { "name": "alpha", "color": "red" }, { "name": "beta", "color": "green" } ] } }
#[tracing::instrument(level = "trace")]
fn merge(a: &mut Value, b: &Value) {
    match (a, b) {
        (Value::Object(ref mut a), &Value::Object(ref b)) => {
            // debug!(message = "Merging objects", a = ?a, b = ?b);
            for (k, v) in b {
                merge(a.entry(k).or_insert(Value::Null), v);
            }
        }
        (Value::Array(ref mut a), &Value::Array(ref b)) => {
            if a.len() != b.len() {
                fn stringify(v: &[Value]) -> String {
                    v.iter().map(|x| x.to_string()).collect_vec().join(",")
                }
                error!(
                    message = "Arrays are of differing lengths",
                    a = stringify(a).as_str(),
                    b = stringify(b).as_str()
                );
                panic!(
                    "Arrays are of differing lengths. {} != {}",
                    a.len(),
                    b.len()
                );
            }
            // debug!(message = "Merging arrays", a = ?a, b = ?b);
            let iter = a.iter_mut();
            let mut out_of_bounds_point = None;
            for (i, v) in iter.enumerate() {
                if i >= b.len() {
                    out_of_bounds_point = Some(i);
                    break;
                    // let a1 = a.clone();
                }
                merge(v, &b[i]);
            }
            if let Some(i) = out_of_bounds_point {
                error!(message = "Index out of bounds", index = i, length = b.len(), a = ?a, b = ?b);
                panic!("Index out of bounds")
            }
        }
        (Value::Array(ref mut _a), &Value::Object(ref _b)) => {
            error!(message = "Tried to merge Array and Object", a = ?_a, b = ?_b);
            panic!("Tried to merge Array and Object");
        }
        (a, b) => {
            // debug!(message = "Merging two Values; clone B into A", a = ?a, b = ?b);
            *a = b.clone();
        }
    }
}

// NB: These tests don't run under WASM. They pass when I ran them under regular Rust, though.
#[cfg(test)]
mod serde_json_value_updater_test {
    use super::*;
    #[test]
    #[should_panic]
    fn it_should_panic_when_merging_array_and_object() {
        let mut object: Value = serde_json::from_str(r#"{"foo":"bar"}"#).unwrap();
        let array: Value = serde_json::from_str(r#"[1,2,3]"#).unwrap();
        object.merge(&array);
    }

    #[test]
    fn it_should_merge_two_objects() {
        let mut a: Value = serde_json::from_str(r#"{"foo":"bar"}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"baz":"bak"}"#).unwrap();
        a.merge(&b);
        assert_eq!(serde_json::json!({"foo":"bar","baz":"bak"}), a);
    }

    #[test]
    fn it_should_merge_two_arrays_of_objects() {
        let mut a: Value =
            serde_json::from_str(r#"[{"name":"Moe"},{"name":"Curly"},{"name":"Larry"}]"#).unwrap();
        let b: Value = serde_json::from_str(
            r#"[{"occupation":"Stooge 1"},{"occupation":"Stooge 2"},{"occupation":"Stooge 3"}]"#,
        )
        .unwrap();
        a.merge(&b);
        // dbg!(&a);
        assert_eq!(
            serde_json::json!([{"name":"Moe", "occupation": "Stooge 1"},
            {"name":"Curly", "occupation": "Stooge 2"} ,
            {"name":"Larry", "occupation": "Stooge 3"}
            ]),
            a
        );
    }

    #[test]
    fn it_should_merge_two_nested_arrays_of_objects() {
        let mut a: Value = serde_json::from_str(
            r#"{"data": { "stooges": [{"name":"Moe"},{"name":"Curly"},{"name":"Larry"}]}}"#,
        )
        .unwrap();
        let b: Value = serde_json::from_str(
            r#"{"data": { "stooges": [{"occupation":"Stooge 1"},{"occupation":"Stooge 2"},{"occupation":"Stooge 3"}]}}"#,
        )
        .unwrap();
        a.merge(&b);
        // dbg!(&a);
        assert_eq!(
            serde_json::json!({"data": { "stooges": [{"name":"Moe", "occupation": "Stooge 1"},
            {"name":"Curly", "occupation": "Stooge 2"} ,
            {"name":"Larry", "occupation": "Stooge 3"}
            ]}}),
            a
        );
    }

    #[test]
    fn it_should_merge_two_deeply_nested_objects() {
        let mut a: Value = serde_json::from_str(
            r#"{
  "data":{
    "stoogeAnalysis":{
      "hair":{
        "Moe":{"type":"straight"},
        "Larry":{"type":"frizzy"},
        "Curly":{"type":"none"}
      }
    }
  }
}"#,
        )
        .unwrap();
        let b: Value = serde_json::from_str(
            r#"{
  "data":{
    "stoogeAnalysis":{
      "hair":{
        "Moe":{"color":"black"},
        "Larry":{"color":"red"},
        "Curly":{"color":"none"}
      }
    }
  }
}"#,
        )
        .unwrap();
        a.merge(&b);
        assert_eq!(
            serde_json::json!({
            "data":{
              "stoogeAnalysis":{
                "hair":{
                  "Moe":{"color":"black", "type": "straight"},
                  "Larry":{"color":"red", "type": "frizzy"},
                  "Curly":{"color":"none", "type": "none"}
                }
              }
            }}),
            a
        );
    }
}
