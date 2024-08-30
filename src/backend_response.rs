// Copyright 2024 Aurelia Peters
//
// This file is part of GraphQL Cacher.
// 
// GraphQL Cacher is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
// 
// GraphQL Cacher is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
// 
// You should have received a copy of the GNU General Public License along with GraphQL Cacher. If not, see <https://www.gnu.org/licenses/>. 
use anyhow::{bail, Result};
use fastly::{Request, Response};
use itertools::Itertools;
use serde_json::Value;

#[derive(Debug)]
pub struct GraphqlErrors {
    pub(crate) errors: Vec<GraphqlError>,
}
impl std::fmt::Display for GraphqlErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<String>>()
                .join("\n")
        )
    }
}
impl std::error::Error for GraphqlErrors {}

// foo bar baz

#[derive(Debug, PartialEq, Eq)]
pub struct GraphqlError {
    pub value: Value,
}
impl std::fmt::Display for GraphqlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let code = self
            .value
            .pointer("/extensions/code")
            .map_or("", |v| v.as_str().unwrap_or(""));

        write!(
            f,
            "({}) {}. Locations: {}",
            code, self.value["message"], self.value["locations"]
        )
    }
}

pub struct BackendResponse {
    pub response: Response,
    pub json_data: Value,
}

impl BackendResponse {
    // fn get_header_all_str(&self, header: &str) -> Vec<&str> {
    //     self.response.get_header_all_str(header)
    // }

    pub fn get_backend_request(&self) -> Option<&Request> {
        self.response.get_backend_request()
    }

    pub fn graphql_errors(&self) -> Vec<GraphqlError> {
        self.json_data.get("errors").map_or(vec![], |errors| {
            errors
                .as_array()
                .unwrap()
                .iter()
                .map(|v| GraphqlError {
                    value: v.to_owned(),
                })
                .collect_vec()
        })
    }

    pub fn new(mut response: Response) -> Result<Self> {
        match response.get_content_type() {
            Some(ct) => match ct.essence_str() {
                "application/json" => (),
                _ => {
                    let request = response.take_backend_request().unwrap();
                    let status = response.get_status().as_u16();
                    let _span = tracing::error_span!(
                        "Unexpected content type from backend",
                        content_type = ct.essence_str(),
                        "request.url" = request.get_url_str(),
                        "request.method" = request.get_method().as_str(),
                        status
                    )
                    .entered();
                    if status >= 500 {
                        // let orig_request = self.response.get_backend_request().unwrap();
                        // println!("Original request\n--\n\n{:?}\n--\n", &orig_request);
                        tracing::error!(
                            message = "Got 5XX error from backend",
                            response_content = response.take_body_str().as_str(),
                        );
                    } else {
                        tracing::error!(
                            message = format!(
                                "Unexpected content type from server: \"{}\". Status {}",
                                ct, status
                            )
                            .as_str(),
                        );
                    }

                    bail!(
                        "Unexpected content type from server: \"{}\". Status {}",
                        ct,
                        status
                    );
                }
            },
            _ => bail!("Empty \"Content-Type\" header received from backend"),
        };
        let json_data = response.take_body_json()?;
        Ok(Self {
            response,
            json_data,
        })
    }
}
