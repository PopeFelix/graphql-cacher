// Copyright 2024 Aurelia Peters
//
// This file is part of GraphQL Cacher.
// 
// GraphQL Cacher is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
// 
// GraphQL Cacher is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
// 
// You should have received a copy of the GNU General Public License along with GraphQL Cacher. If not, see <https://www.gnu.org/licenses/>. 
use fastly::Request;
use std::collections::hash_map::{IntoIter, Keys};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Headers<'a> {
    inner: HashMap<&'a str, Vec<&'a str>>,
}

impl<'a> Headers<'a> {
    pub fn from_request(req: &'a Request, headers_to_pass: &[&'a str]) -> Self {
        let mut inner = HashMap::new();
        for header in headers_to_pass {
            let val = req.get_header_all_str(*header);
            if !val.is_empty() {
                inner.insert(*header, val);
            }
        }
        Self { inner }
    }

    pub fn get_header(&self, name: &str) -> Option<&Vec<&str>> {
        self.inner.get(name)
    }

    pub fn get_headers(&self) -> Keys<&str, Vec<&str>> {
        self.inner.keys()
    }

    // fn iter(&self) -> HeadersIter<'_> {
    //     HeadersIter(self)
    // }
}

impl<'a> IntoIterator for Headers<'a> {
    type Item = (&'a str, Vec<&'a str>);
    type IntoIter = IntoIter<&'a str, Vec<&'a str>>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

// struct HeadersIter<'a>(&'a Headers<'a>);

// impl<'a> Iterator for HeadersIter<'a> {
//     type Item = (&'a &'a str, &'a Vec<&'a str>);

//     fn next(&mut self) -> Option<Self::Item> {
//         let n = self.0.inner.iter().next();
//         n
//     }
// }
