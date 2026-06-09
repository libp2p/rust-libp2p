use std::{
    net::{Ipv4Addr, Ipv6Addr},
    time::Duration,
};

use url::Url;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{AbortSignal, Request, RequestInit, Response};

use crate::web_context::WebContext;

/// The Cloudflare DoH JSON endpoint.
pub const CLOUDFLARE: &str = "https://cloudflare-dns.com/dns-query";

/// The Google DoH JSON endpoint.
pub const GOOGLE: &str = "https://dns.google/resolve";

// TODO: Add other DoH endpoints for default?

// DNS record type codes as used by the DoH JSON API.
const TYPE_A: u16 = 1;
const TYPE_AAAA: u16 = 28;
const TYPE_TXT: u16 = 16;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Policy for resolving `/dns`, `/dns4` and `/dns6` components to IP addresses.
///
/// `/dnsaddr` is always resolved regardless of this policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DnsResolution {
    /// Resolve `/dns*` to `/ip*` only for addresses that require a literal IP,
    /// i.e. those containing a `/webrtc-direct`. Otherwise pass `/dns*` through.
    #[default]
    Auto,
    /// Always resolve `/dns*` to `/ip*`.
    Always,
    /// Never resolve `/dns*`
    Never,
}

/// Configuration for the DNS-over-HTTPS Resolver.
#[derive(Debug, Clone)]
pub struct Config {
    /// A DoH endpoint that answers GET queries in the JSON (`application/dns-json`) format.
    endpoint: String,
    /// Resoluton for how `/dns`, `/dns4` and `/dns6` components are handled.
    dns_resolution: DnsResolution,
    /// Timeout for a single DoH request.
    timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self::cloudflare()
    }
}

impl Config {
    /// Creates a configuration pointing at a custom DoH JSON endpoint.
    pub fn new(endpoint: impl Into<String>) -> Self {
        Config {
            endpoint: endpoint.into(),
            dns_resolution: DnsResolution::default(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Resolve via Cloudflare (see `https://cloudflare-dns.com/dns-query`).
    pub fn cloudflare() -> Self {
        Config::new(CLOUDFLARE)
    }

    /// Resolve via Google (see `https://dns.google/resolve`).
    pub fn google() -> Self {
        Config::new(GOOGLE)
    }

    /// Sets the [`DnsResolution`] policy for `/dns`, `/dns4` and `/dns6`
    /// components.
    pub fn dns_resolution(mut self, policy: DnsResolution) -> Self {
        self.dns_resolution = policy;
        self
    }

    /// Sets the timeout for a single DoH request. Defaults to 10 seconds.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// A DNS resolver that performs lookups over HTTPS (DoH) using the browser's
/// `fetch` API. This is the only way to resolve arbitrary DNS records (in
/// particular the TXT records behind `/dnsaddr`) from within a browser.
#[derive(Debug, Clone)]
pub(crate) struct Resolver {
    config: Config,
}

impl Resolver {
    pub(crate) fn new(config: Config) -> Self {
        Resolver { config }
    }

    /// The configured [`DnsResolution`] policy for `/dns*` components.
    pub(crate) fn dns_resolution(&self) -> DnsResolution {
        self.config.dns_resolution
    }

    pub(crate) async fn ipv4_lookup(&self, name: &str) -> Result<Vec<Ipv4Addr>, ResolveError> {
        Ok(self
            .query(name, TYPE_A)
            .await?
            .iter()
            .filter_map(|d| d.parse::<Ipv4Addr>().ok())
            .collect())
    }

    pub(crate) async fn ipv6_lookup(&self, name: &str) -> Result<Vec<Ipv6Addr>, ResolveError> {
        Ok(self
            .query(name, TYPE_AAAA)
            .await?
            .iter()
            .filter_map(|d| d.parse::<Ipv6Addr>().ok())
            .collect())
    }

    pub(crate) async fn txt_lookup(&self, name: &str) -> Result<Vec<String>, ResolveError> {
        Ok(self
            .query(name, TYPE_TXT)
            .await?
            .iter()
            .map(|d| unquote_txt(d))
            .collect())
    }

    /// Performs a single DoH lookup, returning the `data` field of every answer
    /// whose record type matches `qtype`. An empty result means the lookup
    /// succeeded but no matching records exist.
    async fn query(&self, name: &str, qtype: u16) -> Result<Vec<String>, ResolveError> {
        let url = build_query_url(&self.config.endpoint, name, qtype)?;
        let body = doh_get(url.as_str(), self.config.timeout).await?;
        let response: DohResponse =
            serde_json::from_str(&body).map_err(|e| ResolveError::Parse(e.to_string()))?;
        if response.status != 0 {
            return Err(ResolveError::Status(response.status));
        }
        Ok(response
            .answer
            .into_iter()
            .filter(|a| a.kind == qtype)
            .map(|a| a.data)
            .collect())
    }
}

/// The relevant subset of a DoH JSON response.
#[derive(serde::Deserialize)]
struct DohResponse {
    #[serde(rename = "Status")]
    status: u32,
    #[serde(default, rename = "Answer")]
    answer: Vec<DohAnswer>,
}

#[derive(serde::Deserialize)]
struct DohAnswer {
    #[serde(rename = "type")]
    kind: u16,
    data: String,
}

fn build_query_url(endpoint: &str, name: &str, qtype: u16) -> Result<Url, ResolveError> {
    let mut url = Url::parse(endpoint).map_err(|e| ResolveError::Url(e.to_string()))?;
    url.query_pairs_mut()
        .append_pair("name", name)
        .append_pair("type", &qtype.to_string());
    Ok(url)
}

/// Issues the actual `fetch` for a DoH JSON query and returns the response body.
async fn doh_get(url: &str, timeout: Duration) -> Result<String, ResolveError> {
    let opts = RequestInit::new();
    opts.set_signal(Some(&AbortSignal::timeout_with_f64(
        timeout.as_millis() as f64
    )));

    let request = Request::new_with_str_and_init(url, &opts).map_err(js_error)?;
    request
        .headers()
        .set("accept", "application/dns-json")
        .map_err(js_error)?;

    let context = WebContext::new()
        .ok_or_else(|| ResolveError::Fetch("no browser global scope available".to_owned()))?;

    let response = JsFuture::from(context.fetch_with_request(&request))
        .await
        .map_err(js_error)?;
    let response: Response = response
        .dyn_into()
        .map_err(|_| ResolveError::Fetch("fetch did not return a Response".to_owned()))?;

    if !response.ok() {
        return Err(ResolveError::Http(response.status()));
    }

    let text = JsFuture::from(response.text().map_err(js_error)?)
        .await
        .map_err(js_error)?;
    text.as_string()
        .ok_or_else(|| ResolveError::Fetch("response body was not a string".to_owned()))
}

/// DoH JSON returns TXT records wrapped in literal double quotes; strip a single
/// surrounding pair if present.
fn unquote_txt(s: &str) -> String {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
        .to_owned()
}

fn js_error(value: JsValue) -> ResolveError {
    ResolveError::Fetch(format!("{value:?}"))
}

/// Errors that can occur while resolving a DNS over HTTPSs.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ResolveError {
    /// The `fetch` call or response handling failed (network error, wrong
    /// global scope, non-string body, etcc).
    #[error("DNS-over-HTTPS request failed: {0}")]
    Fetch(String),
    /// The DoH endpoint returned a non-success HTTP status.
    #[error("DNS-over-HTTPS request returned HTTP status {0}")]
    Http(u16),
    /// The DoH endpoint returned a DNS error status.
    #[error("DNS query failed with status {0}")]
    Status(u32),
    /// The DoH response could not be parsed.
    #[error("failed to parse DNS-over-HTTPS response: {0}")]
    Parse(String),
    /// The configured DoH endpoint is not a valid URL.
    #[error("invalid DNS-over-HTTPS endpoint URL: {0}")]
    Url(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unquote_txt_strips_single_surrounding_pair() {
        assert_eq!(unquote_txt("\"dnsaddr=/dns4/foo\""), "dnsaddr=/dns4/foo");
        assert_eq!(unquote_txt("dnsaddr=/dns4/foo"), "dnsaddr=/dns4/foo");
        assert_eq!(unquote_txt("\"\""), "");
    }

    #[test]
    fn parses_doh_json() {
        let body = r#"{"Status":0,"Answer":[
            {"name":"example.com.","type":1,"TTL":60,"data":"1.2.3.4"},
            {"name":"example.com.","type":5,"TTL":60,"data":"cname.example.com."}
        ]}"#;
        let response: DohResponse = serde_json::from_str(body).unwrap();
        assert_eq!(response.status, 0);
        let a_records: Vec<_> = response
            .answer
            .into_iter()
            .filter(|a| a.kind == TYPE_A)
            .map(|a| a.data)
            .collect();
        assert_eq!(a_records, vec!["1.2.3.4".to_owned()]);
    }
}
