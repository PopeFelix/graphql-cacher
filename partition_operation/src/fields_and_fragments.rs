// Copyright 2024 Aurelia Peters
//
// This file is part of GraphQL Operation Partitioner.
// 
// GraphQL Operation Partitioner is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
// 
// GraphQL Operation Partitioner is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
// 
// You should have received a copy of the GNU General Public License along with GraphQL Cacher. If not, see <https://www.gnu.org/licenses/>. 
//! Provides views into a GraphQL selection set of fields, fragment spreads, and inline fragments
use itertools::Itertools;
use graphql_parser::query::{Field, FragmentSpread, InlineFragment, Text, Selection, SelectionSet};
pub(crate) trait FieldsAndFragments<'a, T: Text<'a>> {
    fn fields(&self) -> Vec<&Field<'a, T>>;
    fn fragment_spreads(&self) -> Vec<&FragmentSpread<'a, T>>;
    fn inline_fragments(&self) -> Vec<&InlineFragment<'a, T>>;
}

impl<'a> FieldsAndFragments<'a, &'a str> for SelectionSet<'a, &'a str> {

    /// Return a list of references to all items in this Selection Set which 
    /// are Fields (https://spec.graphql.org/June2018/#sec-Language.Fields)
    fn fields(&self) -> Vec<&Field<'a, &'a str>> {
        self.items
            .iter()
            .filter_map(|selection| match selection {
                Selection::Field(f) => Some(f),
                _ => None,
            })
            .collect_vec()
    }

    /// Return a list of references to all items in this Selection Set which 
    /// are Fragment Spreads (https://spec.graphql.org/June2018/#sec-Fragment-Spreads)
    fn fragment_spreads(&self) -> Vec<&FragmentSpread<'a, &'a str>> {
        self.items
            .iter()
            .filter_map(|selection| match selection {
                Selection::FragmentSpread(f) => Some(f),
                _ => None,
            })
            .collect_vec()
    }

    /// Return a list of references to all items in this Selection Set which 
    /// are Inline Fragments (https://spec.graphql.org/June2018/#sec-Inline-Fragments)
    fn inline_fragments(&self) -> Vec<&InlineFragment<'a, &'a str>> {
        self.items
            .iter()
            .filter_map(|selection| match selection {
                Selection::InlineFragment(f) => Some(f),
                _ => None,
            })
            .collect_vec()
    }
}

