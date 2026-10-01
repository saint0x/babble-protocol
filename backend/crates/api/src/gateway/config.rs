use axum::http::HeaderValue;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// Explicit local-origin provisioning. Public deployments need separate DNS/TLS
/// provisioning; they must not expose this loopback-only listener as an API proxy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GatewayConfig {
    pub(crate) bind_addr: SocketAddr,
    pub(crate) ancestors: HeaderValue,
}

impl GatewayConfig {
    pub fn loopback(bind_addr: SocketAddr, parent_origins: &[String]) -> Result<Self, String> {
        if bind_addr.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) || bind_addr.port() == 0 {
            return Err("bundle gateway requires 127.0.0.1 and an explicit nonzero port".into());
        }
        if parent_origins.is_empty() || parent_origins.len() > 32 {
            return Err("bundle gateway requires 1 to 32 explicit parent origins".into());
        }
        for origin in parent_origins {
            let uri = url::Url::parse(origin).map_err(|_| "invalid bundle parent origin")?;
            if origin.len() > 2048
                || !matches!(uri.scheme(), "https" | "http")
                || !uri.username().is_empty()
                || uri.password().is_some()
                || uri.host_str().is_none_or(|host| host.ends_with('.'))
                || uri.path() != "/"
                || uri.query().is_some()
                || uri.fragment().is_some()
                || *origin != uri.origin().ascii_serialization()
            {
                return Err("bundle parent origin must be an exact HTTP(S) origin without path or credentials".into());
            }
        }
        let parents = parent_origins.join(" ");
        if parents.len() > 4096 {
            return Err("bundle parent origin header exceeds 4096 bytes".into());
        }
        let ancestors =
            HeaderValue::from_str(&parents).map_err(|_| "invalid bundle parent origins")?;
        Ok(Self {
            bind_addr,
            ancestors,
        })
    }

    pub fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }
}
