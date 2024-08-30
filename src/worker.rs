// Copyright 2024 Aurelia Peters
//
// This file is part of GraphQL Cacher.
// 
// GraphQL Cacher is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
// 
// GraphQL Cacher is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
// 
// You should have received a copy of the GNU General Public License along with GraphQL Cacher. If not, see <https://www.gnu.org/licenses/>. 
use crate::backend::Backend;
use crate::backend_response::BackendResponse;
use crate::graphql_request;
use crate::headers::Headers;
use crate::json_merge;
// use crate::{graphql_request, HeaderMap};
use anyhow::bail;
use anyhow::{Error, Result};
use fastly::http::request::PendingRequest;
use fastly::Response;
use graphql_parser::query::{FragmentDefinition, OperationDefinition};
use graphql_request::GraphqlRequest;
use json_merge::Merge;
use partition_operation::Partition;
use serde_json::{json, Value};
use tracing::{debug, debug_span, error};
use uuid::Uuid;

#[derive(Debug)]
pub struct Worker<'a> {
    backend: &'a Backend,
    path: &'a str,
    headers: &'a Headers<'a>,
    variables: &'a Option<Value>,
    request_id: Uuid,
    is_subscriber: bool,
    fragments: Vec<FragmentDefinition<'a, &'a str>>,
}

impl<'a> Worker<'a> {
    pub fn new(
        backend: &'a Backend,
        path: &'a str,
        headers: &'a Headers<'a>,
        variables: &'a Option<serde_json::Value>,
        is_subscriber: bool,
        fragments: Vec<FragmentDefinition<'a, &'a str>>,
    ) -> Self {
        let request_id = Uuid::new_v4();
        Worker {
            backend,
            path,
            headers,
            variables,
            request_id,
            is_subscriber,
            fragments,
        }
    }

    // #[instrument]
    pub fn process_operation(
        &self,
        operation: OperationDefinition<'a, &'a str>,
    ) -> Result<Response> {
        let mut requests = self.get_requests(operation)?;

        debug!("Got {} requests from document", requests.len());
        let mut container: Value = serde_json::from_str("{}").unwrap();

        let mut response: Option<Response> = None;

        let mut counter = 0;
        // TODO: isn't this just an iterator?
        while !requests.is_empty() {
            let _span = debug_span!("Request {}", counter);
            let (res, remaining_requests) = Self::select(requests);
            debug!(
                "Request {}: got response, {} remaining requests",
                counter,
                remaining_requests.len()
            );

            match res {
                Ok(res) => {
                    if response.is_none() {
                        debug!("Request {}: populating composite response", counter);
                        response = Some(res.response.clone_without_body());
                    }
                    let request = res.get_backend_request().unwrap();
                    let x_cache = res.response.get_header_all_str("x-cache").join(";");
                    debug!(
                        request.headers.accept =
                            request.get_header_all_str("Accept").join("; ").as_str(),
                        request.headers.content_type = request
                            .get_header_all_str("Content-Type")
                            .join("; ")
                            .as_str(),
                        request.url = request.get_url_str(),
                        request.method = request.get_method_str(),
                        response.headers.accept = res
                            .response
                            .get_header_all_str("Accept")
                            .join("; ")
                            .as_str(),
                        response.headers.content_type = res
                            .response
                            .get_header_all_str("Content-Type")
                            .join("; ")
                            .as_str(),
                        x_cache = x_cache.as_str(),
                        "Request {}: Got response OK",
                        counter // "request.url" = request.get_url_str(),
                                // "request.method" = request.get_method().as_str(),
                                // "request.headers" = ?request_headers,
                                // process_document = true
                    );

                    let graphql_response = &res.json_data;
                    let graphql_errors = res.graphql_errors();

                    if !graphql_errors.is_empty() {
                        debug!("Request {}: Got GraphQL errors!", counter);
                        let request = res.get_backend_request().unwrap();
                        if !container.as_object().unwrap().contains_key("errors") {
                            container["errors"] = json!([]);
                        }
                        let errors = container["errors"].as_array_mut().unwrap();
                        let query: Value = request.get_query().unwrap();
                        debug!(
                            "Request {}: purging cache for URL {}",
                            counter,
                            request.get_url()
                        );
                        self.backend.purge_cache(request.get_url())?;

                        error!(
                            "Request {}: Server reported {} errors",
                            counter,
                            graphql_errors.len()
                        );
                        for (i, error) in graphql_errors.iter().enumerate() {
                            if !errors.contains(&error.value) {
                                error!(
                                    message = format!(
                                        "Error {}/{}: {}",
                                        i + 1,
                                        graphql_errors.len(),
                                        error
                                    )
                                    .as_str(),
                                    "request.url" = request.get_url_str(),
                                    "request.method" = request.get_method().as_str(),
                                    "request.query" = query["query"].to_string().as_str(),
                                    "request.variables" = query["variables"].to_string().as_str(),
                                    "request.operation_name" =
                                        query["operation_name"].to_string().as_str(),
                                    "request.headers.cache-control" = request
                                        .get_header_all_str("cache-control")
                                        .join("; ")
                                        .as_str(),
                                );
                                errors.push(error.value.to_owned());
                            }
                        }
                    } else {
                        debug!("Request {}: No GraphQL errors found", counter);
                        container.merge(graphql_response);
                    }
                    requests = remaining_requests;
                }
                Err(why) => return Err(why),
            }

            // debug!("{} requests remaining", remaining_requests.len());
            // let backend_url = response.get_backend_request().unwrap().get_url().as_str();
            // debug!("Backend URL: {}", backend_url);
            counter += 1;
        }
        let mut response = response.unwrap();
        response.set_body_json(&container)?;

        Ok(response)
    }

    fn select(requests: Vec<PendingRequest>) -> (Result<BackendResponse>, Vec<PendingRequest>) {
        // let _span = debug_span!("select",);
        let (res, remaining_requests) = fastly::http::request::select(requests);
        let response = res.unwrap();
        (BackendResponse::new(response), remaining_requests)
    }

    // #[instrument]
    fn get_requests(
        &self,
        operation: OperationDefinition<'a, &'a str>,
    ) -> Result<Vec<PendingRequest>> {
        match operation.partition_by_path(self.path)? {
            Some((left, right)) => {
                // println!("Left operation (POST) is {}", left);
                // println!("Right operation (GET) is {}", right);
                let left_request =
                    GraphqlRequest::from_operation_definition(left, vec![], self.variables.clone())
                        .post(self.headers)?;

                let right_request = GraphqlRequest::from_operation_definition(
                    right,
                    self.fragments.clone(), // FIXME: Can I get around cloning?
                    self.variables.clone(),
                )
                .get(self.headers, Some(self.is_subscriber))?
                .with_header("x-gql", "true");

                vec![left_request, right_request]
                    .into_iter()
                    .map(|mut request| {
                        let request_id = Uuid::new_v4();
                        let composite_request_id =
                            format!("{}:{}", self.request_id.as_simple(), request_id.as_simple());
                        if !request.contains_header("x-backend-env") {
                            request.set_header("X-Backend-Env", self.backend.env.as_str());
                        }
                        request.set_header("X-Graphql-Cacher-Request-Id", composite_request_id);
                        tracing::debug!(
                            request.method = request.get_method().as_str(),
                            request.url = request.get_url_str(),
                            "Send subquery: {} {}",
                            request.get_method_str(),
                            request.get_url_str()
                        );
                        // if request.get_method_str() == "POST" {
                        //     let mut clone = request.clone_with_body();
                        //     println!("---- BEGIN POST REQUEST ----");
                        //     println!("{} {}", clone.get_method_str(), clone.get_url_str());
                        //     for (header, value) in &clone.headers_as_hash_map() {
                        //         println!("{}: {}", header, value);
                        //     }
                        //     println!();
                        //     let body = clone.take_body_str();
                        //     println!("{}", body);
                        //     println!("---- END POST REQUEST ----");
                        // }
                        self.backend.send_async(request).map_err(Error::from)
                    })
                    .collect::<Result<Vec<PendingRequest>>>()
            }
            None => {
                tracing::error!(
                    "Path \"{}\" did not match any paths in the given operation definition",
                    self.path
                );
                bail!(
                    "Path \"{}\" did not match any paths in the given operation definition",
                    self.path
                )
            }
        }
    }
}
