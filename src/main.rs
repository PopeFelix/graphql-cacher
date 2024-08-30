// Copyright 2024 Aurelia Peters
//
// This file is part of GraphQL Cacher.
// 
// GraphQL Cacher is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
// 
// GraphQL Cacher is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
// 
// You should have received a copy of the GNU General Public License along with GraphQL Cacher. If not, see <https://www.gnu.org/licenses/>. 
use anyhow::Result;
use backend::Backend;
use backend_response::{BackendResponse, GraphqlErrors};
use fastly::http::{Method, StatusCode};
use fastly::limits::RequestLimits;
use fastly::{Error, Request, Response};
use graphql_parser::query::{Definition, FragmentDefinition};
use graphql_parser::{
    parse_query,
    query::{Document, OperationDefinition},
};
use graphql_request::GraphqlRequest;
use itertools::{Either, Itertools};
use lazy_static::lazy_static;
use serde_json::{json, Value};
use std::collections::HashMap;
use tempus_fugit::{measure, Duration};
use tracing::{debug, error, info, info_span, subscriber, warn};
use tracing_subscriber::Registry;
use tracing_subscriber::{filter::LevelFilter, prelude::*};

mod backend;
mod backend_response;
mod graphql_request;
mod headers;
mod json_merge;
mod worker;
use headers::Headers;
use worker::Worker;

use crate::backend::BackendType;

const MAX_HEADER_VALUE_BYTES: usize = 16384;
const SUBSCRIBER_STATUS_QUERY: &str = "{ currentUser { isSportslineSubscriber } }";
const LOGGING_ENDPOINT: &str = "New Relic";
const LOG_LEVEL: LevelFilter = LevelFilter::INFO;
const LONG_QUERY_TIME_MS: i64 = 500; // Queries (that we process) exceeding this length will be logged as "long" queries

pub trait HeaderMap {
    fn headers_as_hash_map(&self) -> HashMap<&str, String>;
}

impl HeaderMap for fastly::Request {
    fn headers_as_hash_map(&self) -> HashMap<&str, String> {
        let mut headers = HashMap::new();
        for header in self.get_header_names_str() {
            let val = self.get_header_all_str(header).join("; ");
            headers.insert(header, val);
        }
        headers
    }
}
impl HeaderMap for fastly::Response {
    fn headers_as_hash_map(&self) -> HashMap<&str, String> {
        let mut headers = HashMap::new();
        for header in self.get_header_names_str() {
            let val = self.get_header_all_str(header).join("; ");
            headers.insert(header, val);
        }
        headers
    }
}
#[derive(Copy, Clone, PartialEq, Eq)]
enum HowToProcess {
    DoNotProcess,
    Partition,
    DoNotPartition,
}
impl std::fmt::Display for HowToProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let stringval = match self {
            HowToProcess::DoNotProcess => "Do Not Process",
            HowToProcess::Partition => "Partition",
            HowToProcess::DoNotPartition => "Do Not Partition",
        };
        write!(f, "{}", stringval)
    }
}

type OperationsAndFragments<'a> = (
    Vec<OperationDefinition<'a, &'a str>>,
    Vec<FragmentDefinition<'a, &'a str>>,
);

#[derive(Copy, Clone)]
struct ProcessingInstruction<'a> {
    path: Option<&'a str>,
    how_to_process: HowToProcess,
}
impl Default for ProcessingInstruction<'_> {
    fn default() -> Self {
        Self {
            path: None,
            how_to_process: HowToProcess::DoNotProcess,
        }
    }
}

impl<'b> ProcessingInstruction<'b> {
    fn do_not_partition() -> Self {
        Self {
            how_to_process: HowToProcess::DoNotPartition,
            path: None,
        }
    }
    fn partition(do_not_cache: &'b str) -> Self {
        Self {
            how_to_process: HowToProcess::Partition,
            path: Some(do_not_cache),
        }
    }

    /// Get the appropriate processing instruction for the given GraphQL request. If the
    /// query string contained in the request has been parsed, the operation and fragment
    /// definitions extracted from the parsed document will also be returned.
    ///
    /// This method will first look at the query parameter passed in the GraphQL request.
    /// If this parameter is empty or not present, the "Do Not Process" instruction will
    /// be returned. Next the method will look at the operation name parameter passed in
    /// the request. If this parameter is empty or not present, the query parameter will
    /// be parsed. If the query contains more than one operation definition, the "Do Not
    /// Process" instruction will be returned. Otherwise, the operation name will be taken
    /// from the operation definition. Regardless of the source of this value, the operation
    /// name will be checked against the PROCESSING_INSTRUCTIONS lookup. If the operation
    /// name is present, the associated processing instruction will be returned. Otherwise
    /// the "Do Not Process" instruction will be returned.
    ///
    /// Processing instruction rules:
    /// 1) GraphQL request has query string? If yes, proceed to #2. If no, instruction
    ///    is "Do Not Process"
    /// 2) GraphQL request has operation name parameter? If yes, proceed to #4. If no,  
    ///    proceed to #3.
    /// 3) Operation name present in parsed query? If yes, Proceed to #4. If no,  
    ///    instruction is "Do Not Process"
    /// 4) Operation name present in PROCESSING_INSTRUCTIONS? If yes, instruction is the
    ///    value associated with the operation name. If no, instruction is "Do Not Process"
    fn from_graphql_request(
        graphql_request: &GraphqlRequest,
    ) -> Result<(Self, Option<OperationsAndFragments>)> {
        let mut operations_and_fragments = None;
        let processing_instruction = match graphql_request.query.as_ref() {
            Some(query) => {
                if graphql_request.is_persisted_query() {
                    debug!(graphql_request = ?graphql_request, "Request is a persisted query. Do not process");
                    Self::default()
                } else {
                    match graphql_request.operation_name.as_ref() {
                        Some(operation_name) => PROCESSING_INSTRUCTIONS
                            .get(operation_name.as_str())
                            .map_or_else(Self::default, |x| x.to_owned()),
                        None => {
                            let document = parse_query::<&str>(query.as_str())?;

                            operations_and_fragments =
                                Some(into_operations_and_fragments(document));
                            Self::from_operations(&operations_and_fragments.as_ref().unwrap().0[..])
                        }
                    }
                }
            }
            None => Self::default(),
        };
        Ok((processing_instruction, operations_and_fragments))
    }

    fn from_operations<'a>(operations: &[OperationDefinition<'a, &'a str>]) -> Self {
        if operations.len() != 1 {
            info!(
                "Multiple operations ({}) found in query. Do not process.",
                operations.len()
            );
            return Self::default();
        }

        match &operations[0] {
            OperationDefinition::SelectionSet(_) => Self::default(),
            OperationDefinition::Query(query) => {
                match query.name {
                    Some(name) => match PROCESSING_INSTRUCTIONS.get(name) {
                        // cloning instruction is inefficient, but it's pretty cheap
                        Some(instruction) => *instruction,
                        None => Self::default(),
                    },
                    None => Self::default(),
                }
            }
            // Do not process if there is anything other than a query or a bare selection set in the parsed document
            _ => Self::default(),
        }
    }
}

lazy_static! {
    static ref PROCESSING_INSTRUCTIONS: HashMap<&'static str, ProcessingInstruction<'static>> = {
        let mut map = HashMap::new();
        map.insert(
            "MatchupAnalysisQuery",
            ProcessingInstruction::partition("matchupAnalysis.somePrediction"),
        );
        map.insert(
            "PushNotificationSubscriptions",
            ProcessingInstruction::do_not_partition(),
        );
        map.insert("GameInstances", ProcessingInstruction::do_not_partition());
        map.insert(
            "CentralBracketsState",
            ProcessingInstruction::do_not_partition(),
        );
        map.insert(
            "CentralGameInstancesQuery",
            ProcessingInstruction::do_not_partition(),
        );
        map.insert(
            "CentralTeamsQuery",
            ProcessingInstruction::do_not_partition(),
        );
        map.insert("PoolPeriodQuery", ProcessingInstruction::do_not_partition());
        map.insert("GameInstances", ProcessingInstruction::do_not_partition());
        map.insert(
            "FantasyArticlesQuery",
            ProcessingInstruction::do_not_partition(),
        );
        map.insert("AssetSrcQuery", ProcessingInstruction::do_not_partition());
        map.insert(
            "PushNotificationSubscriptions",
            ProcessingInstruction::do_not_partition(),
        );
        map
    };
}

lazy_static! {
    static ref VERSION: String =
        std::env::var("FASTLY_SERVICE_VERSION").unwrap_or_else(|_| String::new());
}

lazy_static! {
    static ref PASS_HEADERS: Vec<&'static str> = vec![
        "cookie",
        "cache-control",
        "x-test-identifier",
        "x-backend-env",
        "authorization",
        "access-control-request-method",
        "access-control-request-headers",
        "origin",
        "content-type",
        "accept"
    ];
}

/*
***************************************************************************************************
********************************************** NOTES **********************************************
***************************************************************************************************
*/

// #[instrument]
fn main() -> Result<(), Error> {
    logging_init();
    RequestLimits::set_max_header_value_bytes(Some(MAX_HEADER_VALUE_BYTES));
    let req = Request::from_client();
    // let mut req = Request::from_client();
    // _print_request(&mut req, "");
    // debug!(request = ?req, "Received request");
    // let mut res = match handle_request(req) {
    let res = match handle_request(req) {
        Ok(res) => {
            // debug!(response = ?res, "Request handled successfully");
            res
        }
        Err(why) => {
            error!(error = ?why, "Error sending request: {}", why);
            Response::from_status(StatusCode::INTERNAL_SERVER_ERROR)
                .with_header("X-Why", format!("{}", &why))
                .with_header("X-Came-From", "edge")
                .with_header("X-GraphQL-Cacher-Version", VERSION.as_str())
                .with_body_text_plain("The application was unable to process the request")
        }
    };
    // res.set_header("X-GraphQL-Cacher-Test-Header", "test test test");
    res.send_to_client();
    Ok(())
}

// #[instrument]
fn handle_request(req: Request) -> Result<Response, Error> {
    // println!("*** Handle request: {:?}", &req);
    let res = match req.get_path() {
        "/graphql" => match req.get_method_str() {
            "GET" => {
                let _span = info_span!("flat_cache").entered();
                // debug!("Flat caching GET request");
                let (res, measurement) = measure!(flat_cache(req));
                let dur = Duration::from(measurement.clone()).num_nanoseconds();
                info!(
                    timing = "true",
                    method = "flat_cache (GET)",
                    durationNs = dur,
                    "Elapsed in flat_cache: {}",
                    measurement
                );
                res
            }
            "POST" => {
                // debug!("Got POST request");
                let content_type = req
                    .get_content_type()
                    .map_or_else(|| "".to_string(), |ct| ct.essence_str().to_string());
                if content_type == "application/json" {
                    // debug!("Content type is JSON; handle request");
                    handle_post(req)
                } else {
                    // debug!(
                    //     "Content type ({}) is not JSON; send unmodified",
                    //     content_type
                    // );
                    send_unmodified(req)
                }
            }
            _ => {
                // debug!(
                //     "Method ({}) is neither GET nor POST; send unmodified",
                //     req.get_method_str()
                // );
                send_unmodified(req)
            }
        },
        _ => {
            // debug!(
            //     "Request path (\"{}\") is not \"/graphql\"; send unmodified",
            //     req.get_path()
            // );
            send_unmodified(req)
        }
    };
    res
}

// #[instrument]
fn handle_post(mut req: Request) -> Result<Response> {
    debug_assert!(req.get_method() == Method::POST, "Got a POST request");
    // let body_json: Value = req.clone_with_body().take_body_json()?;
    // println!("JSON: {}", body_json.to_string());
    let graphql_request: GraphqlRequest = req.take_body_json()?;
    let request_clone = graphql_request.clone();

    let (processing_instruction, mut operations_and_fragments) =
        ProcessingInstruction::from_graphql_request(&request_clone)?;

    let operation_name = match operations_and_fragments {
        Some(ref operations_and_fragments) => {
            let operations = &operations_and_fragments.0;
            match operations[0] {
                OperationDefinition::Query(ref query) => query
                    .name
                    .map_or_else(|| "None".to_string(), |n| n.to_string()),
                _ => "Not a Query".to_string(),
            }
        }
        _ => "None".to_string(),
    };

    // println!(
    //     "Operation: {}. Processing instruction: {}",
    //     operation_name, processing_instruction.how_to_process
    // );
    // debug!(
    //     processing_instruction = processing_instruction.how_to_process.to_string().as_str(),
    //     "Got processing instruction {}",
    //     processing_instruction.how_to_process.to_string().as_str()
    // );

    let (res, measurement) = measure!(match processing_instruction.how_to_process {
        HowToProcess::DoNotProcess => {
            let _span = info_span!("send_unmodified", operation = operation_name).entered();
            // let _span2 = debug_span!(
            //     "Process request",
            //     processing_instruction = "Do Not Process",
            //     process_document = "false"
            // )
            // .entered();
            // debug!("Pass request unmodified");

            // If there is no query with this request (e.g. a persisted query sent via POST), serde_json
            // will serialize the request with the "query" field set to null. This causes the backend to
            // throw an error, so in the case where the request has no query, manually serialize the
            // request to JSON
            if graphql_request.query.is_none() {
                // debug!("No query in GraphQL request. Serialize request manually");
                let mut json_body = json!({});
                if let Some(extensions) = graphql_request.extensions {
                    json_body["extensions"] = extensions
                }
                if let Some(variables) = graphql_request.variables {
                    json_body["variables"] = variables
                }
                if let Some(operation_name) = graphql_request.operation_name {
                    json_body["operationName"] = json!(operation_name)
                }
                req.set_body_json(&json_body)?;
            } else {
                req.set_body_json(&graphql_request)?;
            }
            send_unmodified(req)
        }
        HowToProcess::Partition => {
            debug_assert!(
                graphql_request.query.is_some(),
                "GraphQL request has a query"
            );
            let _span = info_span!("partition", operation = operation_name).entered();
            let backend = Backend::from_request(&req, BackendType::Main)?;
            // let _span = debug_span!(
            //     "Process request",
            //     processing_instruction = "Break Down",
            //     process_document = "true"
            // )
            // .entered();

            if operations_and_fragments.is_none() {
                let document =
                    parse_query::<&str>(graphql_request.query.as_ref().unwrap().as_str())?;

                operations_and_fragments = Some(into_operations_and_fragments(document));
            }

            // debug!(
            //     request.headers = ?req.headers_as_hash_map(),
            //     "Request headers (partition)"
            // );
            info!(
                request.method = req.get_method().as_str(),
                request.url = req.get_url_str(),
                behavior = "partition",
                operation_name = operation_name,
                "Partition request"
            );

            let (mut operations, fragments) = operations_and_fragments.unwrap();
            let headers = Headers::from_request(&req, &PASS_HEADERS);
            // debug!("Headers from request (partition): {:?}", &headers);
            let (is_subscriber, measurement) =
                measure!(get_subscriber_status(&backend, &headers)?);
            let dur = Duration::from(measurement.clone()).num_nanoseconds();
            // let i = processing_instruction.how_to_process.;
            info!(
                timing = "true",
                method = "get_subscriber_status",
                durationNs = dur,
                operation = operation_name,
                instruction = processing_instruction.how_to_process.to_string(),
                "Elapsed in get_subscriber_status: {}",
                measurement
            );

            // debug!(
            //     "Got subscriber status (partition): {}",
            //     &is_subscriber
            // );
            let _span = info_span!("process document").entered();
            let worker = Worker::new(
                &backend,
                processing_instruction.path.unwrap(),
                &headers,
                &graphql_request.variables,
                is_subscriber,
                fragments,
            );
            // debug!("Processing request");

            debug_assert_eq!(operations.len(), 1, "Exactly one operation present");

            let (mut res, measurement) = measure!(worker
                .process_operation(operations.pop().unwrap())
                .map_err(|why| {
                    error!("Process query failed: {}", why);
                    why
                })?);
            let dur = Duration::from(measurement.clone()).num_nanoseconds();
            info!(
                timing = "true",
                method = "process_operation",
                durationNs = dur,
                operation = operation_name,
                instruction = processing_instruction.how_to_process.to_string(),
                "Elapsed in process_operation: {}",
                measurement
            );
            // debug!("Request processed successfully");
            res.set_header("X-Came-From", "edge");
            res.set_header("X-Processed-By-GraphQL-Cacher", "true");
            res.set_header("X-GraphQL-Cacher-Version", VERSION.as_str());
            res.set_header("X-GraphQL-Cacher-Behavior", "partition");
            res.set_header("Cache-Control", "max-age=300, private");

            Ok(res)
        }
        HowToProcess::DoNotPartition => {
            let _span = info_span!("partition", operation = operation_name).entered();
            let headers = Headers::from_request(&req, &PASS_HEADERS);
            let req = graphql_request.get(&headers, None)?;
            let (res, measurement) = measure!(flat_cache(req));
            let dur = Duration::from(measurement.clone()).num_nanoseconds();
            info!(
                timing = "true",
                method = "flat_cache",
                durationNs = dur,
                operation = operation_name,
                instruction = processing_instruction.how_to_process.to_string(),
                "Elapsed in flat_cache: {}",
                measurement
            );
            res
        }
    });
    if processing_instruction.how_to_process != HowToProcess::DoNotProcess {
        let dur = Duration::from(measurement);
        if dur > Duration::milliseconds(LONG_QUERY_TIME_MS) {
            println!(
                "*** LONG QUERY: {} {} ms***",
                &operation_name,
                dur.num_milliseconds(),
            );
            warn!(
                timing = "true",
                method = "handle_post",
                durationNs = dur.num_nanoseconds(),
                operation = operation_name,
                instruction = processing_instruction.how_to_process.to_string(),
                "LONG QUERY: \"{}\" {} ms",
                &operation_name,
                dur.num_milliseconds(),
            )
        }
    }
    res
}

fn logging_init() {
    fn fastly_writer() -> impl std::io::Write {
        fastly::log::Endpoint::from_name(LOGGING_ENDPOINT)
    }
    let subscriber = Registry::default()
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .flatten_event(true)
                .with_line_number(true)
                .with_current_span(true)
                .with_span_list(true)
                .with_target(true)
                .with_writer(fastly_writer),
        )
        .with(LOG_LEVEL);
    subscriber::set_global_default(subscriber).unwrap();

    std::panic::set_hook(Box::new(|panic| {
        println!("CAUGHT PANIC");
        dbg!(&panic);
        // If the panic has a source location, record it as structured fields.
        if let Some(location) = panic.location() {
            // On nightly Rust, where the `PanicInfo` type also exposes a
            // `message()` method returning just the message, we could record
            // just the message instead of the entire `fmt::Display`
            // implementation, avoiding the duplciated location
            error!(
                message = %panic,
                panic.file = location.file(),
                panic.line = location.line(),
                panic.column = location.column(),
            );
        } else {
            error!(message = %panic);
        }
    }));
    // fastly::log::set_panic_endpoint(LOGGING_ENDPOINT).unwrap();
}

// #[instrument]
fn get_subscriber_status(backend: &Backend, headers: &Headers) -> Result<bool> {
    let req = GraphqlRequest {
        query: Some(SUBSCRIBER_STATUS_QUERY.to_string()),
        variables: None,
        operation_name: None,
        extensions: None,
    };
    // debug!(request = ?req, "Requesting subscriber status");

    let res = backend.send(req.get(headers, None)?)?;
    let backend_res = BackendResponse::new(res)?;
    // debug!(
    //     json_data = &backend_res.json_data.to_string().as_str(),
    //     "subscriber status response data: {}",
    //     &backend_res.json_data.to_string().as_str()
    // );
    let errors = backend_res.graphql_errors();
    if errors.is_empty() {
        Ok(
            backend_res.json_data["data"]["currentUser"]["isSportslineSubscriber"]
                .as_bool()
                .unwrap(),
        )
    } else {
        let request = backend_res.response.get_backend_request().unwrap();
        let query: Value = request.get_query().unwrap();
        backend.purge_cache(request.get_url())?;

        error!(
            request.url = request.get_url_str(),
            request.method = request.get_method().as_str(),
            request.headers = ?request.headers_as_hash_map(),
            query = query["query"].to_string().as_str(),
            variables = query["variables"].to_string().as_str(),
            operation_name = query["operation_name"].to_string().as_str(),
            errors = ?errors,
           "Server reported {} errors", errors.len()
        );

        Err(Error::from(GraphqlErrors { errors }))
    }
}

fn into_operations_and_fragments<'a>(
    document: Document<'a, &'a str>,
) -> (
    Vec<OperationDefinition<'a, &'a str>>,
    Vec<FragmentDefinition<'a, &'a str>>,
) {
    document
        .definitions
        .into_iter()
        .partition_map(|def| match def {
            Definition::Operation(x) => Either::Left(x),
            Definition::Fragment(x) => Either::Right(x),
        })
}

// Flat cache a GraphQL GET request. This will send a request unmodified *except* for
// the case where the operation name is "MatchupAnalysisQuery", in which case the
// caller's Sportsline subscriber status will be checked and the result appended to
// the request's query parameters.
// #[instrument]
fn flat_cache(mut req: Request) -> Result<Response> {
    // debug!(
    //     request.headers = ?req.headers_as_hash_map(),
    //     "Request headers (flat cached)"
    // );
    info!(
        request.method = req.get_method().as_str(),
        request.url = req.get_url_str(),
        behavior = "flat_cache",
        "Flat caching request"
    );
    let backend = Backend::from_request(&req, BackendType::Main)?;

    if let Some(operation_name) = req.get_query_parameter("operationName") {
        // println!("Got operation name {}", operation_name);
        // FIXME: I probably shouldn't be hardcoding the operation name here
        if operation_name == "MatchupAnalysisQuery" {
            let headers = Headers::from_request(&req, &PASS_HEADERS);
            let is_subscriber = get_subscriber_status(&backend, &headers)?;
            // println!("Is subscriber? {}", is_subscriber);
            debug!(
                "Got subscriber status (flat_cache): {}",
                &is_subscriber
            );
            let mut query_params: HashMap<String, String> = req.get_query()?;
            query_params.insert("subscriber".to_string(), is_subscriber.to_string());
            req.set_query(&query_params)?;
        }
    }

    // _print_request(&mut req, "FLAT CACHE");

    // debug!(request = ?req, "Sending flat cached request");
    let request_url = req.get_url_str().to_string();
    let mut res = backend.send(req).map_err(|why| {
        error!(
            request_url = request_url.as_str(),
            "Send request failed: {}", why
        );
        why
    })?;
    res.set_header("X-Came-From", "edge");
    res.set_header("X-Processed-By-GraphQL-Cacher", "true");
    res.set_header("X-GraphQL-Cacher-Behavior", "flat cache");
    res.set_header("X-GraphQL-Cacher-Version", VERSION.as_str());

    // _print_response(&mut res, "FLAT CACHE");

    Ok(res)
}

fn send_unmodified(req: Request) -> Result<Response> {
    let backend = Backend::from_request(&req, BackendType::Bypass)?;
    info!(
        request.method = req.get_method().as_str(),
        request.url = req.get_url_str(),
        behavior = "send_unmodified",
        "Send request unmodified"
    );

    let mut res = backend.send(req).map_err(Error::from)?;
    res.set_header("X-Came-From", "edge");
    res.set_header("X-Processed-By-GraphQL-Cacher", "false");
    res.set_header("X-GraphQL-Cacher-Behavior", "send unmodified");
    res.set_header("X-GraphQL-Cacher-Version", VERSION.as_str());
    Ok(res)
}

fn _print_request(req: &mut Request, label: &str) {
    let mut clone = req.clone_with_body();
    println!("----- BEGIN {} REQUEST -----", label);
    println!("{} {}", clone.get_method_str(), clone.get_url_str());
    for (header, value) in &clone.headers_as_hash_map() {
        println!("{}: {}", header, value);
    }
    println!("{}", clone.take_body_str());
    println!("----- END {} REQUEST -----", label);
}

fn _print_response(res: &mut Response, label: &str) {
    let mut clone = res.clone_with_body();
    println!("----- BEGIN {} RESPONSE -----", label);
    for (header, value) in &clone.headers_as_hash_map() {
        println!("{}: {}", header, value);
    }
    println!("{}", clone.take_body_str());
    println!("----- END {} RESPONSE -----", label);
}
