use crate::LocalNode;
use babble_capabilities::{CapabilityCall, CapabilityId, CapabilityReceipt, GrantDecision};
use babble_judgment::JudgmentProvider;
use babble_types::{CapabilityGrantId, ObjectId, Result, Timestamp};
use reqwest::{
    Url,
    blocking::Client,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

const NETWORK_FETCH_CAPABILITY: &str = "babble.network.fetch";
const NETWORK_FETCH_VERSION: u32 = 1;
const MAX_REQUEST_HEADER_BYTES: usize = 16 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NetworkFetchResult {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    pub receipt: CapabilityReceipt,
}

impl<P> LocalNode<P>
where
    P: JudgmentProvider,
{
    pub fn network_fetch(
        &self,
        object_id: &ObjectId,
        method: &str,
        url: &str,
        headers: BTreeMap<String, String>,
        body: Option<Vec<u8>>,
        grant_ids: &[String],
    ) -> Result<NetworkFetchResult> {
        self.check_ready()?;
        self.require_moderation_execution(object_id)?;
        self.require_object(object_id)?;
        let method = FetchMethod::parse(method)?;
        let url = parse_fetch_url(url)?;
        let origin = url_origin(&url)?;
        let headers = validate_headers(headers)?;
        let body = body.unwrap_or_default();
        let request_bytes = fetch_request_bytes(method.as_str(), url.as_str(), &headers, &body)?;
        let receipt = self.authorize_network_fetch(object_id, &origin, grant_ids, request_bytes)?;
        let response = perform_fetch(method, url, headers, body)?;
        if response.body.len() as u64 > receipt.remaining_bytes_per_minute {
            return Err(babble_types::Error::Conflict(format!(
                "network response exceeds remaining byte quota: {} > {}",
                response.body.len(),
                receipt.remaining_bytes_per_minute
            )));
        }
        Ok(NetworkFetchResult {
            status: response.status,
            headers: response.headers,
            body: response.body,
            receipt,
        })
    }

    fn authorize_network_fetch(
        &self,
        object_id: &ObjectId,
        origin: &str,
        grant_ids: &[String],
        requested_bytes: u64,
    ) -> Result<CapabilityReceipt> {
        if grant_ids.is_empty() {
            return Err(babble_types::Error::Conflict(format!(
                "missing capability grant binding for {NETWORK_FETCH_CAPABILITY}@{NETWORK_FETCH_VERSION}"
            )));
        }
        let grant_ids = grant_ids
            .iter()
            .map(|id| {
                let id = CapabilityGrantId::new_unchecked(id.clone());
                id.validate()?;
                Ok(id)
            })
            .collect::<Result<BTreeSet<_>>>()?;
        let capability = CapabilityId::new(NETWORK_FETCH_CAPABILITY)?;
        let grants = self.capability_grants(object_id)?;
        let now = Timestamp::now();
        for grant in &grants {
            if grant_ids.contains(&grant.id)
                && &grant.object_id == object_id
                && grant.capability == capability
                && grant.version == NETWORK_FETCH_VERSION
                && grant.decision == GrantDecision::Approved
                && grant.revoked_at.is_none()
                && grant.expires_at.is_none_or(|expires_at| expires_at > now)
                && scope_allows_origin(&grant.scope, origin)
            {
                return self.capability_broker.authorize_call_with_usage(
                    CapabilityCall {
                        object_id: object_id.clone(),
                        capability,
                        version: NETWORK_FETCH_VERSION,
                        scope: grant.scope.clone(),
                        requested_bytes,
                        realtime_connections: 0,
                    },
                    std::slice::from_ref(grant),
                    &[],
                    now,
                );
            }
        }
        Err(babble_types::Error::Conflict(format!(
            "no active network.fetch grant permits origin {origin}"
        )))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FetchMethod {
    Get,
    Post,
}

impl FetchMethod {
    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "GET" => Ok(Self::Get),
            "POST" => Ok(Self::Post),
            other => Err(babble_types::Error::Conflict(format!(
                "unsupported network.fetch method: {other}"
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

struct FetchedResponse {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

fn perform_fetch(
    method: FetchMethod,
    url: Url,
    headers: HeaderMap,
    body: Vec<u8>,
) -> Result<FetchedResponse> {
    let client = Client::builder()
        .timeout(FETCH_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|err| babble_types::Error::ProviderUnavailable(format!("network client: {err}")))?;
    let request = match method {
        FetchMethod::Get if body.is_empty() => client.get(url),
        FetchMethod::Get => {
            return Err(babble_types::Error::Conflict(
                "GET network.fetch requests must not include a body".to_string(),
            ));
        }
        FetchMethod::Post => client.post(url).body(body),
    }
    .headers(headers);
    let response = request
        .send()
        .map_err(|err| babble_types::Error::ProviderUnavailable(format!("network fetch: {err}")))?;
    let status = response.status().as_u16();
    let headers = response_headers(response.headers())?;
    let body = response
        .bytes()
        .map_err(|err| babble_types::Error::ProviderUnavailable(format!("network body: {err}")))?
        .to_vec();
    Ok(FetchedResponse {
        status,
        headers,
        body,
    })
}

fn parse_fetch_url(value: &str) -> Result<Url> {
    let url = Url::parse(value)
        .map_err(|err| babble_types::Error::Conflict(format!("invalid network.fetch URL: {err}")))?;
    let scheme = url.scheme();
    let valid_scheme = scheme == "https" || (scheme == "http" && is_loopback_host(&url));
    if !valid_scheme || url.username() != "" || url.password().is_some() || url.host_str().is_none()
    {
        return Err(babble_types::Error::Conflict(format!(
            "network.fetch URL is outside supported origins: {value}"
        )));
    }
    Ok(url)
}

fn url_origin(url: &Url) -> Result<String> {
    let host = url
        .host_str()
        .ok_or_else(|| babble_types::Error::Conflict("network.fetch URL has no host".to_string()))?;
    let Some(port) = url.port_or_known_default() else {
        return Err(babble_types::Error::Conflict(
            "network.fetch URL has no port or known default".to_string(),
        ));
    };
    let default_port =
        (url.scheme() == "https" && port == 443) || (url.scheme() == "http" && port == 80);
    if default_port {
        Ok(format!("{}://{}", url.scheme(), host))
    } else {
        Ok(format!("{}://{}:{}", url.scheme(), host, port))
    }
}

fn is_loopback_host(url: &Url) -> bool {
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
}

fn validate_headers(headers: BTreeMap<String, String>) -> Result<HeaderMap> {
    let mut total = 0_usize;
    let mut out = HeaderMap::new();
    for (name, value) in headers {
        let lower = name.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "authorization"
                | "cookie"
                | "host"
                | "connection"
                | "content-length"
                | "transfer-encoding"
                | "proxy-authorization"
        ) {
            return Err(babble_types::Error::Conflict(format!(
                "network.fetch header is not allowed: {name}"
            )));
        }
        total = total
            .checked_add(name.len())
            .and_then(|size| size.checked_add(value.len()))
            .ok_or_else(|| {
                babble_types::Error::Conflict("network header size overflow".to_string())
            })?;
        if total > MAX_REQUEST_HEADER_BYTES {
            return Err(babble_types::Error::Conflict(format!(
                "network.fetch headers exceed limit: {total} > {MAX_REQUEST_HEADER_BYTES}"
            )));
        }
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|err| babble_types::Error::Conflict(format!("invalid header name: {err}")))?;
        let value = HeaderValue::from_str(&value)
            .map_err(|err| babble_types::Error::Conflict(format!("invalid header value: {err}")))?;
        out.insert(name, value);
    }
    Ok(out)
}

fn response_headers(headers: &HeaderMap) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (name, value) in headers {
        if name.as_str().eq_ignore_ascii_case("set-cookie") {
            continue;
        }
        let Ok(value) = value.to_str() else {
            continue;
        };
        out.insert(name.as_str().to_ascii_lowercase(), value.to_string());
    }
    Ok(out)
}

fn fetch_request_bytes(method: &str, url: &str, headers: &HeaderMap, body: &[u8]) -> Result<u64> {
    let header_bytes = headers.iter().try_fold(0_u64, |total, (name, value)| {
        let value_len = u64::try_from(value.as_bytes().len())
            .map_err(|_| babble_types::Error::Conflict("header value too large".to_string()))?;
        total
            .checked_add(name.as_str().len() as u64)
            .and_then(|size| size.checked_add(value_len))
            .ok_or_else(|| {
                babble_types::Error::Conflict("network request size overflow".to_string())
            })
    })?;
    (method.len() as u64)
        .checked_add(url.len() as u64)
        .and_then(|size| size.checked_add(header_bytes))
        .and_then(|size| size.checked_add(body.len() as u64))
        .ok_or_else(|| babble_types::Error::Conflict("network request size overflow".to_string()))
}

fn scope_allows_origin(scope: &Value, origin: &str) -> bool {
    scope
        .get("origins")
        .and_then(Value::as_array)
        .is_some_and(|origins| {
            origins
                .iter()
                .filter_map(Value::as_str)
                .any(|allowed| allowed == origin)
        })
}
