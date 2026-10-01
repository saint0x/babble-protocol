//! URI admission for Surface resources. Admission never verifies fetched bytes.

use crate::Resource;
use babble_types::{Error, Hash, Result};
use url::{Host, ParseError, Url};

#[derive(Debug)]
pub struct ResourceUri<'a> {
    raw: &'a str,
    location: Location<'a>,
}

#[derive(Debug)]
enum Location<'a> {
    Blob(&'a str),
    Relative,
    Http(Url),
}

impl<'a> ResourceUri<'a> {
    /// Accept canonical Babble blobs, safe relative references, HTTPS and loopback HTTP.
    /// Queries and fragments on explicitly declared external references are preserved.
    pub fn parse(raw: &'a str) -> Result<Self> {
        if raw.is_empty()
            || raw
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
        {
            return Err(invalid_uri());
        }
        validate_escapes(raw)?;
        let location = match Url::parse(raw) {
            Ok(url) => {
                let (_, remainder) = raw.split_once("://").ok_or_else(invalid_uri)?;
                let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
                let authority = &remainder[..authority_end];
                if authority.is_empty()
                    || authority.contains('@')
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.host().is_none()
                {
                    return Err(invalid_uri());
                }
                // Check the original path before the URL parser removes dot segments.
                validate_path(reference_path(&remainder[authority_end..]), false)?;
                match url.scheme() {
                    "babble" => {
                        let hash = raw.strip_prefix("babble://blobs/").ok_or_else(invalid_uri)?;
                        if !is_canonical_hash(hash) {
                            return Err(invalid_uri());
                        }
                        Location::Blob(hash)
                    }
                    "https" => Location::Http(url),
                    "http" if is_loopback(&url) => Location::Http(url),
                    _ => return Err(invalid_uri()),
                }
            }
            Err(ParseError::RelativeUrlWithoutBase) => {
                let path = reference_path(raw);
                validate_path(path, true)?;
                if path
                    .split('/')
                    .next()
                    .is_some_and(|segment| segment.contains(':'))
                {
                    return Err(invalid_uri());
                }
                Location::Relative
            }
            Err(_) => return Err(invalid_uri()),
        };
        Ok(Self { raw, location })
    }

    pub fn blob_hash(&self) -> Option<&str> {
        match self.location {
            Location::Blob(hash) => Some(hash),
            _ => None,
        }
    }

    pub fn validate_integrity(&self, integrity: &Hash) -> Result<()> {
        if !is_canonical_hash(integrity.as_str())
            || self
                .blob_hash()
                .is_some_and(|hash| hash != integrity.as_str())
        {
            return Err(Error::Conflict(
                "resource URI requires matching canonical integrity".into(),
            ));
        }
        Ok(())
    }

    /// Match the signed URI exactly, or a known gateway representation of a Babble blob.
    /// Gateway shape binds the requested hash, not the response bytes or server identity.
    pub fn matches_resource(&self, resource: &Resource) -> bool {
        let Ok(declared) = ResourceUri::parse(&resource.uri) else {
            return false;
        };
        if self.validate_integrity(&resource.integrity).is_err()
            || declared.validate_integrity(&resource.integrity).is_err()
        {
            return false;
        }
        if self.raw == declared.raw {
            return true;
        }
        let Some(hash) = declared.blob_hash() else {
            return false;
        };
        let Location::Http(url) = &self.location else {
            return false;
        };
        if url.fragment().is_some() {
            return false;
        }
        let path = url.path();
        let gateway_hash = path
            .strip_prefix("/runtime/surfaces/blobs/")
            .or_else(|| path.strip_prefix("/media/blobs/"));
        if gateway_hash != Some(hash) {
            return false;
        }
        // Both API blob routes require a single media_type selector. Other query
        // parameters cannot establish an alias for a signed Babble blob URI.
        match url.query() {
            None => false,
            Some(_) => {
                let mut pairs = url.query_pairs();
                matches!(pairs.next(), Some((key, value))
                    if key == "media_type" && value == resource.media_type)
                    && pairs.next().is_none()
            }
        }
    }
}

fn invalid_uri() -> Error {
    Error::Conflict("resource URI must be a safe relative reference, canonical Babble blob, HTTPS, or loopback HTTP URL".into())
}

fn is_canonical_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain("localhost")) => true,
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    }
}

fn reference_path(reference: &str) -> &str {
    &reference[..reference.find(['?', '#']).unwrap_or(reference.len())]
}

fn validate_escapes(value: &str) -> Result<()> {
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let decoded = decode_escape(&mut bytes)?;
            if decoded.is_ascii_control() || decoded == b'\\' {
                return Err(invalid_uri());
            }
        }
    }
    Ok(())
}

fn decode_escape(bytes: &mut impl Iterator<Item = u8>) -> Result<u8> {
    let high = bytes
        .next()
        .and_then(|byte| (byte as char).to_digit(16))
        .ok_or_else(invalid_uri)?;
    let low = bytes
        .next()
        .and_then(|byte| (byte as char).to_digit(16))
        .ok_or_else(invalid_uri)?;
    Ok((high * 16 + low) as u8)
}

fn validate_path(path: &str, relative: bool) -> Result<()> {
    if relative && (path.is_empty() || path.starts_with('/')) {
        return Err(invalid_uri());
    }
    for segment in path.split('/') {
        if relative && segment.is_empty() {
            return Err(invalid_uri());
        }
        let mut decoded = Vec::with_capacity(segment.len());
        let mut bytes = segment.bytes();
        while let Some(byte) = bytes.next() {
            let byte = if byte == b'%' {
                decode_escape(&mut bytes)?
            } else {
                byte
            };
            // Reject separators and nested escapes so another decoder cannot turn
            // an admitted path segment into traversal or a different URL component.
            if matches!(byte, b'/' | b'\\' | b'?' | b'#' | b'%') || byte.is_ascii_control() {
                return Err(invalid_uri());
            }
            decoded.push(byte);
        }
        if decoded == b"." || decoded == b".." {
            return Err(invalid_uri());
        }
    }
    Ok(())
}
