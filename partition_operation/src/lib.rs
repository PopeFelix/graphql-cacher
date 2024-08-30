// Copyright 2024 Aurelia Peters
//
// This file is part of GraphQL Operation Partitioner.
// 
// GraphQL Operation Partitioner is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
// 
// GraphQL Operation Partitioner is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
// 
// You should have received a copy of the GNU General Public License along with GraphQL Cacher. If not, see <https://www.gnu.org/licenses/>. 
use anyhow::{Error, Result};
use graphql_parser::{
    query::{Definition, Document, Field, OperationDefinition, Selection, SelectionSet, Text},
    Pos,
};
use itertools::{Either, Itertools};
use regex::Regex;

#[cfg(test)]
mod comparisions;
mod fields_and_fragments;

// https://stackoverflow.com/a/69324393/132319
macro_rules! cast {
    ($target: expr, $pat: path) => {{
        if let $pat(a) = $target {
            // #1
            a
        } else {
            panic!("mismatch variant when cast to {}", stringify!($pat)); // #2
        }
    }};
}

pub(crate) trait Operations<'a, T: Text<'a>> {
    fn operations(self) -> Vec<OperationDefinition<'a, T>>;
}

impl<'a> Operations<'a, &'a str> for Document<'a, &'a str> {
    fn operations(self) -> Vec<OperationDefinition<'a, &'a str>> {
        self.definitions
            .into_iter()
            .filter_map(|def| match def {
                Definition::Operation(op_def) => Some(op_def),
                _ => None,
            })
            .collect_vec()
    }
}

type OperationDefinitionPartition<'a, T> = (OperationDefinition<'a, T>, OperationDefinition<'a, T>);

/// Trait used to partition GraphQL Operations. Note that order is not necessarily preserved in a
/// given selection set
pub trait Partition<'a, T: Text<'a>> {
    /// Partition a GraphQL operation by path. See "Query Path Syntax" in README.md
    fn partition_by_path(self, path: &str) -> Result<Option<OperationDefinitionPartition<'a, T>>>;
}
// TODO: implement Partition for Document

impl<'a> Partition<'a, &'a str> for OperationDefinition<'a, &'a str> {
    /// # Examples: Partition a query
    /// ```
    /// use partition_operation::Partition;
    /// use graphql_parser::parse_query;
    ///
    /// let query = parse_query::<&str>(
    ///     "query MyQuery { myQuery { alpha, beta { one { foo, bar }, two }, gamma } }",
    /// )
    /// .unwrap();
    /// let path = "myQuery.beta.one";
    /// let mut operations = query
    ///     .definitions
    ///     .into_iter()
    ///     .filter_map(|def| match def {
    ///         graphql_parser::query::Definition::Operation(op_def) => Some(op_def),
    ///         _ => None,
    ///     })
    ///     .collect::<Vec<graphql_parser::query::OperationDefinition<&str>>>();
    /// let op = operations.pop().unwrap();
    /// let (left, right) = op.partition_by_path(path).unwrap().unwrap();
    /// println!("{}", left);
    /// println!("{}", right);
    /// dbg!(right.to_string());
    /// let expected_left = r#"query MyQuery {
    ///   myQuery {
    ///     beta {
    ///       one {
    ///         foo
    ///         bar
    ///       }
    ///     }
    ///   }
    /// }
    /// "#;
    /// let expected_right = r#"query MyQuery {
    ///   myQuery {
    ///     alpha
    ///     gamma
    ///     beta {
    ///       two
    ///     }
    ///   }
    /// }
    /// "#;
    /// assert_eq!(expected_left, left.to_string(), "LEFT");
    /// assert_eq!(expected_right, right.to_string(), "RIGHT");
    /// ```
    fn partition_by_path(
        self,
        path: &str,
    ) -> Result<Option<OperationDefinitionPartition<'a, &'a str>>> {
        let elements = validate_path(path)?;

        let partition = match self {
            OperationDefinition::Query(mut query) => {
                let mut selection_set = SelectionSet {
                    span: (Pos::default(), Pos::default()),
                    items: vec![],
                };
                std::mem::swap(&mut query.selection_set, &mut selection_set);

                // NB: selection_set here is the selection set taken from the *query*
                partition_selection_set_by_path(elements, selection_set).map(|(left, right)| {
                    let mut q2 = query.clone();
                    q2.selection_set = right;
                    query.selection_set = left;
                    (
                        OperationDefinition::Query(query),
                        OperationDefinition::Query(q2),
                    )
                })
            }
            OperationDefinition::SelectionSet(selection_set) => {
                partition_selection_set_by_path(elements, selection_set).map(|(left, right)| {
                    (
                        OperationDefinition::SelectionSet(left),
                        OperationDefinition::SelectionSet(right),
                    )
                })
            }
            _ => unimplemented!(),
        };
        Ok(partition)
    }
}

fn partition_selection_set_by_path<'a>(
    mut path: Vec<&str>,
    selection_set: graphql_parser::query::SelectionSet<'a, &'a str>,
    // parent_field: Field<'a, &'a str>
) -> Option<(SelectionSet<'a, &'a str>, SelectionSet<'a, &'a str>)> {
    if path.is_empty() {
        return None;
    }
    let field_name = path.remove(0);

    let mut items = selection_set.items;
    let span = selection_set.span;
    match items.iter().position(|f| {
        if let Selection::Field(field) = f {
            field_name_or_alias_matches(field, field_name)
        } else {
            false
        }
    }) {
        Some(index) => {
            let mut field = cast!(items.remove(index), Selection::Field);

            match path.len() {
                0 => Some((
                    SelectionSet {
                        span: (Pos::default(), Pos::default()),
                        items: vec![Selection::Field(field)],
                    },
                    SelectionSet { items, span },
                )), // Create a new SelectionSet with the remaining items
                _ => {
                    if let Some((inner_selection_set, selection_set)) =
                        partition_selection_set_by_path(path, field.selection_set)
                    {
                        field.selection_set = selection_set;
                        let mut f2 = field.clone();
                        f2.selection_set = inner_selection_set;
                        items.push(Selection::Field(field));
                        let right = SelectionSet { items, span };
                        let left = SelectionSet {
                            span: (Pos::default(), Pos::default()),
                            items: vec![Selection::Field(f2)],
                        };
                        Some((left, right))
                    } else {
                        None
                    }
                }
            }
        }
        None => None,
    }
}

#[derive(Debug)]
pub(crate) struct InvalidElementError {
    element: String,
}

impl std::fmt::Display for InvalidElementError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Invalid element in path: \"{}\"", self.element)
    }
}
impl std::error::Error for InvalidElementError {}

/// Validate the given path string. A valid path consists of one or more valid field names separated by
/// the dot (.) character. A valid field name is a string containing only the characters in the range
/// [a-zA-Z0-9_] and beginning with a character in the range [A-Za-z_]
fn validate_path(path: &str) -> Result<Vec<&str>> {
    // http://spec.graphql.org/October2021/#sec-Names
    let re = Regex::new("^[_A-Za-z][_0-9A-Za-z]*$").unwrap();
    let (elements, invalid): (Vec<_>, Vec<_>) =
        path.split('.').partition_map(|e| match re.is_match(e) {
            true => Either::Left(e),
            false => Either::Right(e),
        });

    if invalid.is_empty() {
        Ok(elements)
    } else {
        Err(Error::from(InvalidElementError {
            element: invalid.get(0).unwrap().to_string(),
        }))
    }
}

/// Returns true if the field name or alias match the search string.
fn field_name_or_alias_matches<'a, T: Text<'a, Value = &'a str>>(
    field: &Field<'a, T>,
    search_str: &str,
) -> bool {
    match field.alias {
        Some(alias) => alias == search_str,
        None => field.name == search_str,
    }
}

#[cfg(test)]
mod tests {
    use crate::comparisions::{compare_queries, compare_selection_sets};
    use crate::fields_and_fragments::FieldsAndFragments;
    use crate::{validate_path, Operations, Partition};
    use anyhow::{Error, Result};
    use graphql_parser::query::Field;
    use graphql_parser::schema::Text;
    use graphql_parser::{
        parse_query,
        query::{Document, OperationDefinition, SelectionSet},
    };
    use lazy_static::lazy_static;
    use rand::Rng;
    use random_string::generate;

    trait CloneField<'a, T: Text<'a>> {
        fn clone_field(&self, field: T) -> Option<Field<'a, T>>;
    }
    impl<'a> CloneField<'a, &'a str> for SelectionSet<'a, &'a str> {
        fn clone_field(&self, field: &str) -> Option<Field<'a, &'a str>> {
            self.fields().into_iter().find(|f| f.name == field).cloned()
        }
    }

    lazy_static! {
        static ref VALID_FIELD_NAME_CHARSET: String = {
            let mut valid_chars = ('A' as u32..='Z' as u32).collect::<Vec<u32>>();
            valid_chars.extend('a' as u32..='z' as u32);
            valid_chars.push('_' as u32);
            valid_chars.extend('0' as u32..='9' as u32);
            unsafe {
                valid_chars
                    .into_iter()
                    .map(|c| std::char::from_u32_unchecked(c))
                    .collect()
            }
        };
    }

    #[test]
    fn validate_path_shows_valid_paths_as_valid() {
        let mut rng = rand::thread_rng();

        let path_length: u8 = rng.gen_range(1..=10);
        let valid_start_charset =
            &VALID_FIELD_NAME_CHARSET[..(VALID_FIELD_NAME_CHARSET.len() - 10)];

        let mut paths = valid_start_charset.bytes().map(|c| {
            let mut elements: Vec<String> = Vec::new();
            for _ in 1..path_length {
                let length: usize = rng.gen_range(1..=10);
                let field_name = format!(
                    "{}{}",
                    c as char,
                    generate(length, VALID_FIELD_NAME_CHARSET.as_str())
                );
                elements.push(field_name);
            }
            elements.join(".")
        });
        let res = paths.try_for_each(|p| validate_path(p.as_str()).map(|_| ()));
        assert!(
            res.is_ok(),
            "Path \"{}\" tests as valid",
            paths.next().unwrap()
        );
    }

    #[test]
    fn validate_path_shows_invalid_paths_as_invalid() {
        let invalid_start = "0abc.def.abc";
        let invalid_end = "abc.def.ghi=";
        let invalid_middle = "abc.d@f.ghi";
        let paths = vec![invalid_start, invalid_end, invalid_middle];
        for path in paths {
            assert!(
                validate_path(path).is_err(),
                "Path \"{}\" tests as invalid",
                path
            );
        }
    }

    #[test]
    fn remove_with_invalid_path_returns_err() {
        let op = parse_query("{ myQuery { alpha } }")
            .unwrap()
            .operations()
            .pop()
            .unwrap();
        assert!(op.partition_by_path("0abc.dedf.ghi").is_err())
    }

    #[test]
    fn partition_with_non_matching_path_returns_none() -> Result<(), anyhow::Error> {
        let op = parse_query("{ myQuery { alpha } }")
            .unwrap()
            .operations()
            .pop()
            .unwrap();

        assert_eq!(op.partition_by_path("myQuery.foo")?, None);
        Ok(())
    }

    #[test]
    fn partition_by_top_level_field_leaves_nothing_on_the_right() -> Result<(), anyhow::Error> {
        let query = "{ myQuery { alpha } }";
        let expected_left = "{ myQuery { alpha } }";
        let expected_right = None;
        partition_by_path_ok("myQuery", query, Some(expected_left), expected_right)?;
        Ok(())
    }

    #[test]
    fn partition_preserves_field_structure() -> Result<()> {
        let query = "{ myQuery { alpha { one, two, three } } }";

        let expected_left = "{ myQuery { alpha { two } } }";
        let expected_right = "{ myQuery { alpha { one, three } } }";
        partition_by_path_ok(
            "myQuery.alpha.two",
            query,
            Some(expected_left),
            Some(expected_right),
        )?;
        Ok(())
    }

    #[test]
    fn partition_preserves_query_name_and_arguments() -> Result<()> {
        let query = r#"query MyQuery($foo: String!, $bar: String!) { 
            myQuery(foo: $foo, bar: $bar) { alpha { one, two { a, b { a1, b1 }, c }, three } } 
        }"#;
        let expected_left = r#"query MyQuery($foo: String!, $bar: String!) { 
            myQuery(foo: $foo, bar: $bar) { alpha { two { b { a1 } } } } 
        }"#;
        let expected_right = r#"query MyQuery($foo: String!, $bar: String!) { 
            myQuery(foo: $foo, bar: $bar) { alpha { one, two { a, b { b1 }, c }, three } } 
        }"#;
        partition_by_path_ok(
            "myQuery.alpha.two.b.a1",
            query,
            Some(expected_left),
            Some(expected_right),
        )?;
        Ok(())
    }

    #[test]
    fn partition_path_refers_to_alias() -> Result<()> {
        let query = "{ myQuery { foo, bar { baz: alpha, bak } } }";
        let path = "myQuery.bar.baz";
        let expected_left = "{ myQuery { bar { baz: alpha } } }";
        let expected_right = "{ myQuery { foo, bar { bak } } }";
        partition_by_path_ok(path, query, Some(expected_left), Some(expected_right))?;
        Ok(())
    }

    #[test]
    fn partition_works_on_matchup_analysis() -> Result<()> {
        let query = include_str!("../fixtures/matchupAnalysis.graphql");
        let expected_left = include_str!("../fixtures/matchupAnalysis-EXPECTED_LEFT.graphql");
        let expected_right = include_str!("../fixtures/matchupAnalysis-EXPECTED_RIGHT.graphql");
        partition_by_path_ok(
            "matchupAnalysis.somePrediction",
            query,
            Some(expected_left),
            Some(expected_right),
        )?;
        Ok(())
    }

    fn partition_by_path_ok<'a>(
        path: &str,
        query: &'a str,
        expected_left: Option<&'a str>,
        expected_right: Option<&'a str>,
    ) -> Result<(), Error> {
        let doc: Document<&str> = parse_query(query)?;
        let mut operations = doc.operations();
        assert_eq!(
            operations.len(),
            1,
            "Document contains exactly one operation"
        );

        let expected_left =
            expected_left.map(|s| parse_query::<&str>(s).unwrap().operations().pop().unwrap());
        let expected_right =
            expected_right.map(|s| parse_query::<&str>(s).unwrap().operations().pop().unwrap());

        let operation = operations.pop().unwrap();
        let res = operation.partition_by_path(path)?;
        match (res, expected_left) {
            (None, None) => {
                if let Some(operation_definition) = expected_right {
                    panic!(
                        "Expected operation definition on RHS, found None. Expected: {}",
                        operation_definition
                    )
                }
            }
            (None, Some(_)) => panic!("The path \"{}\" was not found in query: {}", path, query),
            (Some((operation, _selection_set)), None) => panic!(
                "Got unexpected (expected None) operation definition from path \"{}\": {}",
                operation, path
            ),
            (Some((got_left, got_right)), Some(expected_left)) => {
                if let Some(expected_right) = expected_right {
                    let (both_match, failure_reason) = compare_operation_definition_partitions(
                        (&expected_left, &expected_right),
                        (&got_left, &got_right),
                    );

                    if !both_match {
                        println!("Expected left: {}", &expected_left);
                        println!("Expected right: {}", &expected_right);
                        println!("Got left: {}", &got_left);
                        println!("Got right: {}", &got_right);
                    }
                    assert!(
                        both_match,
                        "{}",
                        failure_reason.unwrap_or_else(|| "".to_string())
                    )
                }
            }
        }
        Ok(())
    }

    fn compare_operation_definition_partitions<'a>(
        expected: (
            &OperationDefinition<'a, &'a str>,
            &OperationDefinition<'a, &'a str>,
        ),
        got: (
            &OperationDefinition<'a, &'a str>,
            &OperationDefinition<'a, &'a str>,
        ),
    ) -> (bool, Option<String>) {
        match expected {
            (
                OperationDefinition::SelectionSet(expected_left),
                OperationDefinition::SelectionSet(expected_right),
            ) => match got {
                (
                    OperationDefinition::SelectionSet(got_left),
                    OperationDefinition::SelectionSet(got_right),
                ) => {
                    let (matches, failure_reason) = compare_selection_sets(expected_left, got_left);
                    if !matches {
                        return (false, failure_reason);
                    }
                    compare_selection_sets(expected_right, got_right)
                }
                _ => panic!("RHS: Expected Selection Set, got Query"),
            },
            (
                OperationDefinition::Query(expected_left),
                OperationDefinition::Query(expected_right),
            ) => match got {
                (OperationDefinition::Query(got_left), OperationDefinition::Query(got_right)) => {
                    let (matches, failure_reason) = compare_queries(expected_left, got_left);
                    if !matches {
                        return (false, failure_reason);
                    }
                    compare_queries(expected_right, got_right)
                }
                _ => panic!("RHS: Expected Query, got Selection Set"),
            },
            _ => unimplemented!(),
        }
    }
}
