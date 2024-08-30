// Copyright 2024 Aurelia Peters
//
// This file is part of GraphQL Operation Partitioner.
// 
// GraphQL Operation Partitioner is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
// 
// GraphQL Operation Partitioner is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
// 
// You should have received a copy of the GNU General Public License along with GraphQL Cacher. If not, see <https://www.gnu.org/licenses/>. 
use graphql_parser::query::{
    Directive, Field, FragmentSpread, InlineFragment, Query, SelectionSet, Value,
    VariableDefinition,
};
use std::collections::HashMap;

use crate::fields_and_fragments::FieldsAndFragments;

pub(crate) fn compare_queries<'a, 'b>(
    expected: &'b Query<'a, &'a str>,
    got: &'b Query<'a, &'a str>,
) -> (bool, Option<String>) {
    // println!("Compare {} and {}", expected, got);
    if expected.name != got.name {
        return (
            false,
            Some(format!(
                "Query names do not match. \"{}\" != \"{}\"",
                expected.name.unwrap_or("None"),
                got.name.unwrap_or("None")
            )),
        );
    }
    let (directives_match, failure_reason) =
        compare_directive_vecs(&expected.directives, &got.directives);
    if !directives_match {
        return (
            false,
            failure_reason.map(|reason| format!("Directives do not match: {}", reason.as_str())),
        );
    }
    let (variable_definitions_match, failure_reason) =
        compare_variable_definition_vecs(&expected.variable_definitions, &got.variable_definitions);
    if !variable_definitions_match {
        return (
            false,
            failure_reason
                .map(|reason| format!("Variable definitions do not match: {}", reason.as_str())),
        );
    }
    compare_selection_sets(&expected.selection_set, &got.selection_set)
}

fn compare_variable_definition_vecs<'a>(
    expected: &[VariableDefinition<'a, &'a str>],
    got: &[VariableDefinition<'a, &'a str>],
) -> (bool, Option<String>) {
    if expected.len() != got.len() {
        return (
            false,
            Some(format!(
                "Number of fragment spreads differ {} != {}",
                &expected.len(),
                &got.len()
            )),
        );
    }

    let mut last_failure_reason: Option<String> = None;
    let all_match = expected.iter().all(|a| {
        got.iter().any(|b| {
            let (matches, failure_reason) = compare_variable_definitions(a, b);
            if !matches {
                last_failure_reason = failure_reason;
                false
            } else {
                true
            }
        })
    });
    (all_match, last_failure_reason)
}

fn compare_variable_definitions<'a, 'b>(
    expected: &'b VariableDefinition<'a, &'a str>,
    got: &'b VariableDefinition<'a, &'a str>,
) -> (bool, Option<String>) {
    if expected.name != got.name {
        return (
            false,
            Some(format!(
                "Variable names do not match. \"{}\" != \"{}\"",
                expected.name, got.name
            )),
        );
    }
    if expected.default_value != got.default_value {
        return (
            false,
            Some(format!(
                "Default values do not match for variable \"{}\". {:?} != {:?}",
                expected.name, expected.default_value, got.default_value
            )),
        );
    }
    if expected.var_type != got.var_type {
        return (
            false,
            Some(format!(
                "Types do not match for variable \"{}\". {:?} != {:?}",
                expected.name, expected.var_type, got.var_type
            )),
        );
    }
    (true, None)
}

pub(crate) fn compare_selection_sets<'a, 'b>(
    expected: &'b SelectionSet<'a, &'a str>,
    got: &'b SelectionSet<'a, &'a str>,
) -> (bool, Option<String>) {
    if expected.items.len() != got.items.len() {
        return (
            false,
            Some(format!(
                "Number of items differ {} != {}",
                &expected.items.len(),
                &got.items.len()
            )),
        );
    }

    let (fragment_spreads_match, failure_reason) =
        compare_fragment_spread_vecs(expected.fragment_spreads(), got.fragment_spreads());
    if !fragment_spreads_match {
        return (fragment_spreads_match, failure_reason);
    }
    let (inline_fragments_match, inline_fragments_failure_reason) =
        compare_inline_fragments(expected.inline_fragments(), got.inline_fragments());
    if !inline_fragments_match {
        return (inline_fragments_match, inline_fragments_failure_reason);
    }
    compare_field_vecs(expected.fields(), got.fields())
}

pub(crate) fn compare_inline_fragments<'a, 'b>(
    expected: Vec<&'b InlineFragment<'a, &'a str>>,
    got: Vec<&'b InlineFragment<'a, &'a str>>,
) -> (bool, Option<String>) {
    if expected.len() != got.len() {
        return (
            false,
            Some(format!(
                "Number of inline fragments differ {} != {}",
                &expected.len(),
                &got.len()
            )),
        );
    }
    if !expected.is_empty() {
        todo!("Handle inline fragments")
    }
    (true, None)
}

pub(crate) fn compare_fragment_spread_vecs<'a, 'b>(
    expected: Vec<&'b FragmentSpread<'a, &'a str>>,
    got: Vec<&'b FragmentSpread<'a, &'a str>>,
) -> (bool, Option<String>) {
    if expected.len() != got.len() {
        return (
            false,
            Some(format!(
                "Number of fragment spreads differ {} != {}",
                &expected.len(),
                &got.len()
            )),
        );
    }

    let mut last_failure_reason: Option<String> = None;
    let fragment_spreads_match = expected.iter().all(|fragment_spread_a| {
        got.iter().any(|fragment_spread_b| {
            if fragment_spread_a.fragment_name != fragment_spread_b.fragment_name {
                last_failure_reason = Some(format!(
                    "Fragment spread \"{}\" missing",
                    fragment_spread_a.fragment_name
                ));
                false
            } else {
                true
            }
        })
    });
    (fragment_spreads_match, last_failure_reason)
}

pub(crate) fn compare_field_vecs<'a, 'b>(
    expected: Vec<&'b Field<'a, &'a str>>,
    got: Vec<&'b Field<'a, &'a str>>,
) -> (bool, Option<String>) {
    if expected.len() != got.len() {
        return (
            false,
            Some(format!(
                "Number of fields differ {} != {}",
                &expected.len(),
                &got.len()
            )),
        );
    }
    let mut last_failure_reason: Option<String> = None;

    // This is not the most efficient way of doing this
    let fields_match = expected.iter().all(|expected| {
        got.iter().any(|got| {
            let (field_matches, failure_reason) = compare_fields(expected, got);
            last_failure_reason = failure_reason;
            field_matches
        })
    });
    (fields_match, last_failure_reason)
}

pub(crate) fn compare_fields<'a, 'b>(
    expected: &'b Field<'a, &'a str>,
    got: &'b Field<'a, &'a str>,
) -> (bool, Option<String>) {
    if expected.name != got.name {
        return (
            false,
            Some(format!(
                "Field names do not match. \"{}\" != \"{}\"",
                expected.name, got.name
            )),
        );
    }

    if expected.alias != got.alias {
        return (
            false,
            Some(format!(
                "Field aliases do not match. \"{}\" != \"{}\"",
                expected.alias.unwrap_or(""),
                got.alias.unwrap_or("")
            )),
        );
    }

    let (directives_match, failure_reason) =
        compare_directive_vecs(&expected.directives, &got.directives);
    if !directives_match {
        return (
            false,
            failure_reason.map(|reason| format!("Directives do not match: {}", reason.as_str())),
        );
    }

    let (args_match, failure_reason) = compare_argument_vecs(&expected.arguments, &got.arguments);
    if !args_match {
        return (false, failure_reason);
    }

    let (selection_sets_match, failure_reason) =
        compare_selection_sets(&expected.selection_set, &got.selection_set);
    (
        selection_sets_match,
        failure_reason.map(|s| format!("Selection sets do not match: {}", s.as_str())),
    )
}

fn compare_directive_vecs<'a>(
    expected: &[Directive<'a, &'a str>],
    got: &[Directive<'a, &'a str>],
) -> (bool, Option<String>) {
    if expected.len() != got.len() {
        return (
            false,
            Some(format!(
                "Number of fields differ {} != {}",
                &expected.len(),
                &got.len()
            )),
        );
    }
    let mut last_failure_reason: Option<String> = None;

    // This is not the most efficient way of doing this
    let vecs_match = expected.iter().all(|expected| {
        got.iter().any(|got| {
            let (directive_matches, failure_reason) = compare_directives(expected, got);
            last_failure_reason = failure_reason;
            directive_matches
        })
    });
    (vecs_match, last_failure_reason)
}

fn compare_argument_vecs<'a>(
    expected: &[(&'a str, Value<'a, &'a str>)],
    got: &[(&'a str, Value<'a, &'a str>)],
) -> (bool, Option<String>) {
    let mut got_map = HashMap::new();
    for (key, val) in got {
        got_map.insert(key, val);
    }

    if expected.len() != got.len() {
        return (
            false,
            Some(format!(
                "Expected {} arguments, got {} arguments",
                expected.len(),
                got.len()
            )),
        );
    }

    let mut failure_reason: Option<String> = None;
    let args_match = expected
        .iter()
        .all(|(key, expected)| match got_map.get(key) {
            Some(got) => {
                if *got != expected {
                    failure_reason = Some(format!(
                        "Values for argument \"{}\" do not match. {} != {}",
                        key, expected, got
                    ));
                    false
                } else {
                    true
                }
            }
            None => {
                failure_reason = Some(format!(
                    "Expected argument to be present for key \"{}\", but none was found",
                    key
                ));
                false
            }
        });
    (args_match, failure_reason)
}

fn compare_directives<'a, 'b>(
    expected: &'b Directive<'a, &'a str>,
    got: &'b Directive<'a, &'a str>,
) -> (bool, Option<String>) {
    if expected.name != got.name {
        return (
            false,
            Some(format!(
                "Directive names do not match. \"{}\" != \"{}\"",
                expected.name, got.name
            )),
        );
    }
    let (args_match, failure_reason) = compare_argument_vecs(&expected.arguments, &got.arguments);
    (
        args_match,
        failure_reason.map(|reason| {
            format!(
                "Argument mismatch for directive \"{}\": {}",
                expected.name,
                reason.as_str()
            )
        }),
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        comparisions::{compare_field_vecs, compare_fields, compare_selection_sets},
        Operations,
    };
    use anyhow::Result;
    use graphql_parser::{
        query::{parse_query, Field, OperationDefinition, Selection, SelectionSet},
        Pos,
    };

    use super::compare_queries;
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

    #[test]
    fn functionally_identical_selection_sets_are_equivalent() {
        let q1 = r#"{ 
            myQuery { 
                alpha
                beta {
                    one
                    two
                }
                gamma {
                    ...fsOne
                }
            }
        }"#;
        let q2 = r#"{ myQuery { alpha, beta { one, two }, gamma { ...fsOne } } }"#;
        let op1 = parse_query(q1).unwrap().operations().pop().unwrap();
        let op2 = parse_query(q2).unwrap().operations().pop().unwrap();
        let ss1 = match op1 {
            OperationDefinition::Query(query) => query.selection_set,
            OperationDefinition::SelectionSet(ss) => ss,
            _ => unimplemented!(),
        };
        let ss2 = match op2 {
            OperationDefinition::Query(query) => query.selection_set,
            OperationDefinition::SelectionSet(ss) => ss,
            _ => unimplemented!(),
        };
        let (matches, failure_reason) = compare_selection_sets(&ss1, &ss2);
        assert!(
            matches,
            "{}",
            failure_reason.unwrap_or_else(|| "".to_string()).as_str()
        );
    }

    #[test]
    fn functionally_identical_selection_sets_are_equivalent_regardless_of_order() {
        let q1 = r#"{ 
            myQuery { 
                beta {
                    one
                    two
                }
                alpha
                gamma {
                    ...fsOne
                }
            }
        }"#;
        let q2 = r#"{ myQuery { alpha, beta { one, two }, gamma { ...fsOne } } }"#;
        let op1 = parse_query(q1).unwrap().operations().pop().unwrap();
        let op2 = parse_query(q2).unwrap().operations().pop().unwrap();
        let ss1 = match op1 {
            OperationDefinition::Query(query) => query.selection_set,
            OperationDefinition::SelectionSet(ss) => ss,
            _ => unimplemented!(),
        };
        let ss2 = match op2 {
            OperationDefinition::Query(query) => query.selection_set,
            OperationDefinition::SelectionSet(ss) => ss,
            _ => unimplemented!(),
        };
        let (matches, failure_reason) = compare_selection_sets(&ss1, &ss2);
        assert!(
            matches,
            "{}",
            failure_reason.unwrap_or_else(|| "".to_string()).as_str()
        );
    }

    #[test]
    fn not_functionally_identical_selection_sets_are_not_equivalent() {
        let q1 = r#"{ 
            myQuery { 
                alpha
                beta {
                    one
                    two
                }
                gamma {
                    ...fsOne
                }
            }
        }"#;
        let q2 = r#"{ myQuery { alpha, iota { one, two }, gamma { ...fsOne } } }"#;
        let op1 = parse_query(q1).unwrap().operations().pop().unwrap();
        let op2 = parse_query(q2).unwrap().operations().pop().unwrap();
        let ss1 = match op1 {
            OperationDefinition::Query(query) => query.selection_set,
            OperationDefinition::SelectionSet(ss) => ss,
            _ => unimplemented!(),
        };
        let ss2 = match op2 {
            OperationDefinition::Query(query) => query.selection_set,
            OperationDefinition::SelectionSet(ss) => ss,
            _ => unimplemented!(),
        };
        let (matches, failure_reason) = compare_selection_sets(&ss1, &ss2);
        assert!(!matches, "Selection sets did not match");
        assert_ne!(failure_reason, None, "Got a failure reason");
    }

    #[test]
    fn fields_with_same_name_and_different_selection_sets_do_not_match() {
        let f1: Field<&str> = Field {
            name: "field1",
            position: Pos::default(),
            alias: None,
            arguments: vec![],
            directives: vec![],
            selection_set: SelectionSet {
                span: (Pos::default(), Pos::default()),
                items: vec![],
            },
        };
        let mut f2 = f1.clone();
        f2.selection_set
            .items
            .push(graphql_parser::query::Selection::Field(f1.clone()));

        let (matches, failure_reason) = compare_fields(&f1, &f2);
        assert!(!matches, "Fields did not match");
        assert_ne!(
            failure_reason.clone(),
            None,
            "Got a failure reason: {}",
            failure_reason.unwrap_or_else(|| "".to_string()).as_str()
        );
    }

    #[test]
    fn fields_with_same_name_and_same_selection_sets_match() {
        let mut f1: Field<&str> = Field {
            name: "field1",
            position: Pos::default(),
            alias: None,
            arguments: vec![],
            directives: vec![],
            selection_set: SelectionSet {
                span: (Pos::default(), Pos::default()),
                items: vec![],
            },
        };
        let mut f2 = f1.clone();
        let mut f3 = f1.clone();
        f3.name = "field2";
        let mut f4 = f1.clone();
        f4.name = "field3";

        f1.selection_set
            .items
            .push(graphql_parser::query::Selection::Field(f3.clone()));
        f1.selection_set
            .items
            .push(graphql_parser::query::Selection::Field(f4.clone()));
        f2.selection_set
            .items
            .push(graphql_parser::query::Selection::Field(f3.clone()));
        f2.selection_set
            .items
            .push(graphql_parser::query::Selection::Field(f4.clone()));

        let (matches, failure_reason) = compare_fields(&f1, &f2);
        assert!(matches, "Fields matched");
        assert_eq!(
            failure_reason.clone(),
            None,
            "Unexpected failure reason: {}",
            failure_reason.unwrap_or_else(|| "".to_string()).as_str()
        );
    }

    #[test]
    fn field_vecs_match_regardless_of_order() {
        let base: Field<&str> = Field {
            name: "field1",
            position: Pos::default(),
            alias: None,
            arguments: vec![],
            directives: vec![],
            selection_set: SelectionSet {
                span: (Pos::default(), Pos::default()),
                items: vec![],
            },
        };
        let f1 = base.clone();
        let mut f2 = base.clone();
        f2.name = "field2";
        let mut f3 = base.clone();
        f3.name = "field3";

        let f1a = f1.clone();
        let f2a = f2.clone();
        let f3a = f3.clone();

        let v1 = vec![&f1, &f2, &f3];
        let v2 = vec![&f3a, &f1a, &f2a];
        let (matches, failure_reason) = compare_field_vecs(v1, v2);
        assert!(matches, "Field vecs matched");
        assert_eq!(
            failure_reason.clone(),
            None,
            "Unexpected failure reason: {}",
            failure_reason.unwrap_or_else(|| "".to_string()).as_str()
        );
    }

    #[test]
    fn fields_with_same_name_and_same_arguments_match() -> Result<()> {
        let f1 = cast!(
            cast!(
                parse_query("{ field(arg1: $arg1, arg2: $arg2) }")?
                    .operations()
                    .pop()
                    .unwrap(),
                OperationDefinition::SelectionSet
            )
            .items
            .pop()
            .unwrap(),
            Selection::Field
        );
        let f2 = cast!(
            cast!(
                parse_query("{ field(arg2: $arg2, arg1: $arg1) }")?
                    .operations()
                    .pop()
                    .unwrap(),
                OperationDefinition::SelectionSet
            )
            .items
            .pop()
            .unwrap(),
            Selection::Field
        );
        let (matches, failure_reason) = compare_fields(&f1, &f2);
        assert!(matches, "Fields matched");
        assert_eq!(
            failure_reason.clone(),
            None,
            "Unexpected failure reason: {}",
            failure_reason.unwrap_or_else(|| "".to_string()).as_str()
        );
        Ok(())
    }

    #[test]
    fn fields_with_same_name_and_different_arguments_do_not_match() -> Result<()> {
        let f1 = cast!(
            cast!(
                parse_query("{ field(arg1: $arg1, arg2: $arg2) }")?
                    .operations()
                    .pop()
                    .unwrap(),
                OperationDefinition::SelectionSet
            )
            .items
            .pop()
            .unwrap(),
            Selection::Field
        );
        let f2 = cast!(
            cast!(
                parse_query("{ field(arg2: $arg2, arg3: $arg1) }")?
                    .operations()
                    .pop()
                    .unwrap(),
                OperationDefinition::SelectionSet
            )
            .items
            .pop()
            .unwrap(),
            Selection::Field
        );
        let (matches, failure_reason) = compare_fields(&f1, &f2);
        assert!(!matches, "Fields did not match");
        assert_ne!(
            failure_reason.clone(),
            None,
            "{}",
            failure_reason.unwrap_or_else(|| "".to_string()).as_str()
        );
        Ok(())
    }

    #[test]
    fn semantically_identical_queries_match() -> Result<()> {
        let q1 = cast!(
            parse_query::<&str>(
                "query MyQuery($arg1: ID) { myQuery(arg1: $arg1) { alpha { one, two } } }",
            )?
            .operations()
            .pop()
            .unwrap(),
            OperationDefinition::Query
        );
        let q2 = cast!(
            parse_query::<&str>(
                r#"query MyQuery($arg1: ID) { 
            myQuery(arg1: $arg1) { 
                alpha { 
                    one 
                    two 
                } 
            } 
        }"#,
            )?
            .operations()
            .pop()
            .unwrap(),
            OperationDefinition::Query
        );

        let (matches, failure_reason) = compare_queries(&q1, &q2);
        assert!(matches, "Queries matched");
        assert_eq!(
            failure_reason.clone(),
            None,
            "Unexpected failure reason: {}",
            failure_reason.unwrap_or_else(|| "".to_string()).as_str()
        );
        Ok(())
    }

    #[test]
    fn semantically_different_queries_do_not_match() -> Result<()> {
        let q1 = cast!(
            parse_query::<&str>(
                "query MyQuery($arg1: ID) { myQuery(arg1: $arg1) { alpha { one, two } } }",
            )?
            .operations()
            .pop()
            .unwrap(),
            OperationDefinition::Query
        );
        let q2 = cast!(
            parse_query::<&str>(
                r#"query MyQuery($arg1: String) { 
            myQuery(arg1: $arg1) { 
                alpha { 
                    one 
                    two 
                } 
            } 
        }"#,
            )?
            .operations()
            .pop()
            .unwrap(),
            OperationDefinition::Query
        );

        let (matches, failure_reason) = compare_queries(&q1, &q2);
        assert!(!matches, "Queries did not match");
        assert_ne!(
            failure_reason.clone(),
            None,
            "{}",
            failure_reason.unwrap_or_else(|| "".to_string()).as_str()
        );
        Ok(())
    }
}
