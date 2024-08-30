// Copyright 2024 Aurelia Peters
//
// This file is part of GraphQL Cacher.
// 
// GraphQL Cacher is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
// 
// GraphQL Cacher is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
// 
// You should have received a copy of the GNU General Public License along with GraphQL Cacher. If not, see <https://www.gnu.org/licenses/>. 
use std::collections::BTreeMap;

use crate::headers::Headers;
use fastly::{http::HeaderValue, Error, Request};
use graphql_parser::query::{Definition, Document, FragmentDefinition, OperationDefinition};
use serde::{Deserialize, Serialize};
use serde_json::Value;
// use tracing::debug;
// use tracing::instrument;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GraphqlRequest {
    pub query: Option<String>,
    pub variables: Option<Value>,
    pub operation_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Value>,
}
impl GraphqlRequest {
    // #[instrument (level="trace")]
    pub fn from_operation_definition<'a>(
        op_def: OperationDefinition<'a, &'a str>,
        fragments: Vec<FragmentDefinition<'a, &'a str>>,
        variables: Option<Value>,
    ) -> Self {
        //        println!("In GraphqlRequest::from_operation_definition");
        let operation_name = match op_def {
            OperationDefinition::Query(ref query) => query.name.map(|s| s.to_string()),
            _ => None,
        };
        //        println!("Operation name: {:?}", &operation_name);
        let mut definitions = vec![Definition::Operation(op_def)];
        definitions.extend(fragments.into_iter().map(Definition::Fragment));
        let document = Document { definitions };
        Self {
            query: Some(document.to_string()),
            variables,
            operation_name,
            extensions: None,
        }
    }

    /// Returns true if this GraphQL request is a persisted query
    pub fn is_persisted_query(&self) -> bool {
        // println!("In GraphqlRequest::is_persisted_query");
        match self.extensions.as_ref() {
            Some(extensions) => !extensions["persistedQuery"].is_null(),
            _ => false,
        }
    }

    // #[instrument (level="trace")]
    pub fn get(
        self,
        headers: &Headers,
        is_subscriber: Option<bool>,
    ) -> Result<Request, Error> {
        // println!("*** In GraphqlRequest::get. Headers: {:?}\nSubscriber? {:?}", &headers, &is_subscriber);
        let mut query_params = BTreeMap::new();
        if let Some(query) = self.query {
            query_params.insert("query", query);
        }
        if let Some(variables) = self.variables {
            query_params.insert("variables", variables.to_string());
        }
        if let Some(extensions) = self.extensions {
            query_params.insert("extensions", extensions.to_string());
        }
        if let Some(is_subscriber) = is_subscriber {
            query_params.insert("subscriber", is_subscriber.to_string());
        }

        let operation_name = self.operation_name.unwrap_or_else(|| "".to_string());
        // println!("Set surrogate key {}", operation_name);
        // debug!("Set surrogate key {}", operation_name);
        // use a dummy URL here; the real URL will be supplied on sending by the backend
        let mut request = Request::get("https://localhost/graphql")
            .with_query(&query_params)?
            .with_surrogate_key(HeaderValue::from_str(operation_name.as_str())?)
            .with_header("X-Operation-Name", operation_name.as_str());

        for header in headers.get_headers() {
            let values = headers.get_header(*header).unwrap();
            for value in values {
                // debug!("Set header \"{}\": \"{}\"", header, value);
                // println!("Set header \"{}\": \"{}\"", header, value);
                request.append_header(header.to_owned(), value.to_owned());
            }
        }
        Ok(request)
    }

    // #[instrument (level="trace")]
    pub fn post(&mut self, headers: &Headers) -> Result<Request, Error> {
        // println!("In GraphqlRequest::post, headers {:?}", &headers);
        // use a dummy URL here; the real URL will be supplied on sending by the backend
        let mut request = Request::post("https://localhost/graphql").with_body_json(&self)?;

        for header in headers.get_headers() {
            let values = headers.get_header(*header).unwrap();
            for value in values {
                // debug!("Set header \"{}\": \"{}\"", header, value);
                //      println!("Set header \"{}\": \"{}\"", header, value);
                request.append_header(header.to_owned(), value.to_owned());
            }
        }
        Ok(request)
    }
}

impl std::fmt::Display for GraphqlRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", serde_json::to_string(self).unwrap())
    }
}
