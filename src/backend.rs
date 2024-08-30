use anyhow::{bail, Result};
use fastly::{
    http::{request::PendingRequest, Url},
    Error, Request, Response,
};
use tracing::info;

use crate::HeaderMap;

const BACKEND_URL_MAIN: &str = "https://graphql-cacher.prod.backend.tld";
const BACKEND_URL_BYPASS_DEV: &str = "https://bypass.dev.backend.tld";
const BACKEND_URL_BYPASS_QA: &str = "https://bypass.qa.backend.tld";
const BACKEND_URL_BYPASS_PROD: &str = "https://bypass.prod.backend.tld";
const DEFAULT_ENV: &str = "qa";

#[derive(Debug)]
pub enum BackendType {
    Main,
    Bypass,
}
impl std::fmt::Display for BackendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendType::Main => write!(f, "main"),
            BackendType::Bypass => write!(f, "bypass"),
        }
    }
}
#[derive(Debug)]
pub struct Backend {
    pub name: &'static str,
    pub url: Url,
    pub env: String,
}
impl Backend {
    /// Create an instance of the "main" backend (i.e. the backend that we send
    /// partitioned and flat cached requests to)
    pub fn main(env: &str) -> Self {
        Backend {
            name: "BACKEND_GRAPHQL_SHIELD",
            env: env.to_string(),
            url: Url::parse(BACKEND_URL_MAIN).unwrap(),
        }
    }

    /// Create an instance of the "bypass" backend (i.e. the backend that we send
    /// unprocessed requests to)
    pub fn bypass(env: &str) -> Result<Self> {
        match env.to_ascii_lowercase().as_str() {
            "dev" => Ok(Backend {
                name: "BACKEND_BYPASS_DEV",
                url: Url::parse(BACKEND_URL_BYPASS_DEV).unwrap(),
                env: env.to_string(),
            }),
            "qa" => Ok(Backend {
                name: "BACKEND_BYPASS_QA",
                url: Url::parse(BACKEND_URL_BYPASS_QA).unwrap(),
                env: env.to_string(),
            }),
            "prod" => Ok(Backend {
                name: "BACKEND_BYPASS_PROD",
                url: Url::parse(BACKEND_URL_BYPASS_PROD).unwrap(),
                env: env.to_string(),
            }),
            _ => bail!(
                "Unrecognized value \"{}\" for env; expected one of \"dev\", \"qa\", or \"prod\".",
                &env
            ),
        }
    }

    /// Create an instance of this class from the 'X-Backend-Env' header of the given Request
    pub fn from_request(req: &Request, ty: BackendType) -> Result<Self> {
        let res = match req.get_header_str("X-Backend-Env") {
            Some(val) => match ty {
                BackendType::Main => Ok(Self::main(val)),
                BackendType::Bypass => Self::bypass(val),
            },
            None => {
                let default = match ty {
                    BackendType::Main => Self::main(DEFAULT_ENV),
                    BackendType::Bypass => Self::bypass(DEFAULT_ENV)?,
                };
                info!(
                    "Backend: No \"X-Backend-Env\" header present, defaulting to {}",
                    default.url
                );
                Ok(default)
            }
        };
        match res {
            Ok(backend) => {
                tracing::debug!(
                    backend_env = backend.env.as_str(),
                    backend_url = backend.url.to_string().as_str(),
                    backend_name = backend.name,
                    "Got {} backend OK.",
                    ty
                );
                Ok(backend)
            }
            Err(why) => {
                tracing::error!(
                    error = ?why,
                    "Unable to initialize {} backend: {}", ty, why
                );
                Err(why)
            }
        }
    }

    /// Send a blocking request. The request URL will be rewritten such that
    /// the host portion is the backend host, the scheme is https, and the port
    /// is 443.
    // #[instrument]
    pub fn send(&self, mut req: Request) -> Result<Response> {
        req.remove_header("host");
        // println!("URL: {}", req.get_url_str());
        let url = req.get_url_mut();
        tracing::debug!("Got request URL: {}", &url);

        url.set_host(self.url.host_str())?;
        url.set_scheme("https").unwrap();
        url.set_port(Some(443)).unwrap();
        tracing::debug!("Modified request URL: {}", &url);

        tracing::debug!(
            message = "Sending request (blocking)",
            "request.method" = req.get_method().as_str(),
            "request.url" = req.get_url_str(),
            "request.headers" = ?req.headers_as_hash_map()
        );
        match req.send(self.name) {
            Ok(res) => {
                tracing::debug!("Request sent OK (blocking)");
                Ok(res)
            }
            Err(why) => {
                tracing::error!(error = ?why, "Error sending request (blocking): {}", why);
                Err(Error::from(why))
            }
        }
    }

    /// Send a non-blocking request. The request URL will be rewritten such that
    /// the host portion is the backend host, the scheme is https, and the port
    /// is 443.
    // #[instrument]
    pub fn send_async(&self, mut req: Request) -> Result<PendingRequest> {
        req.remove_header("host");
        // println!("URL: {}", req.get_url_str());
        let url = req.get_url_mut();
        tracing::debug!("Got request URL");

        url.set_host(self.url.host_str())?;
        url.set_scheme("https").unwrap();
        url.set_port(Some(443)).unwrap();
        tracing::debug!("Modified request URL");

        tracing::debug!(
            message = "Sending request (async)",
            "request.method" = req.get_method().as_str(),
            "request.url" = req.get_url_str(),
            "request.headers" = ?req.headers_as_hash_map()
        );
        match req.send_async(self.name) {
            Ok(res) => {
                tracing::debug!("Request sent OK (async)");
                Ok(res)
            }
            Err(why) => {
                tracing::error!(error = ?why, "Error sending request (async): {}", why);
                Err(Error::from(why))
            }
        }
    }

    pub fn purge_cache(&self, url: &Url) -> Result<()> {
        // See https://developer.fastly.com/reference/api/purging/#purge-single-url
        let request = Request::new("PURGE", url);
        // debug!(message = "PURGE single URL", url = url.as_str());
        match self.send(request) {
            Ok(mut res) => {
                let status_code = res.get_status().as_u16();
                if (200..400).contains(&status_code) {
                    tracing::debug!(url = url.as_str(), "Purged {} OK", url);
                    // debug!("PURGE OK, status {}", status_code);
                    Ok(())
                } else {
                    let response_body = res.take_body_str();
                    tracing::error!(
                        message = "PURGE request failed",
                        status = status_code,
                        url = url.as_str(),
                        "response" = response_body.as_str()
                    );
                    bail!(
                        "PURGE request failed for {}. Server reported error {}",
                        url,
                        status_code
                    )
                }
            }
            Err(why) => {
                tracing::error!(
                    url = url.as_str(),
                    error = ?why,
                    "Failed to send purge request: {}", why
                );
                Err(why)
            }
        }
    }
}
