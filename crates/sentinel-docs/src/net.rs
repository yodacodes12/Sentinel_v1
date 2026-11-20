//! The only module in Sentinel that talks to the network (pinned by the
//! safety suite). Used solely for opt-in document discovery and
//! user-requested downloads.
//!
//! Protections, all tested:
//! - HTTPS only, default port only, no credentials in URLs, bounded length.
//! - No request to loopback, private, link-local, carrier-NAT, multicast,
//!   documentation or other non-public addresses: host names are checked
//!   before the request, and the resolver refuses non-public addresses at
//!   connection time, so DNS rebinding cannot reach the local network.
//! - Redirects are not followed automatically; each hop is re-validated
//!   (at most [`MAX_REDIRECTS`]).
//! - robots.txt is honoured per host; a robots.txt the server fails to
//!   deliver means "do not crawl" (RFC 9309); an unreachable host stops the
//!   request with the real cause (timeout, unreachable).
//! - One request per host per configured interval; HTTP 429 stops the
//!   provider for the session.
//! - Response bodies are bounded; the body is streamed, never buffered
//!   whole for downloads.
//! - No proxy, cookies, compression or authentication.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use url::Url;

pub const MAX_REDIRECTS: u32 = 4;
pub const MAX_URL_CHARS: usize = 2048;
pub const MAX_ROBOTS_BYTES: u64 = 256 * 1024;
pub const USER_AGENT: &str =
    "Sentinel-DocumentDiscovery/0.1 (Linux desktop application; user-initiated)";

/// Shared cancellation flag of a discovery session or download.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NetError {
    #[error("invalid address: {0}")]
    InvalidUrl(String),
    #[error("blocked: {0}")]
    Blocked(String),
    #[error("the request timed out")]
    Timeout,
    #[error("the server answered HTTP {0}")]
    Http(u16),
    #[error("the server asked Sentinel to slow down (HTTP 429)")]
    RateLimited,
    #[error("the response is larger than the {0}-byte limit")]
    TooLarge(u64),
    #[error("cancelled")]
    Cancelled,
    #[error("the source could not be reached: {0}")]
    Unreachable(String),
    #[error("too many redirects")]
    TooManyRedirects,
    #[error("the site's robots.txt does not allow this address")]
    RobotsDisallowed,
    #[error("invalid response: {0}")]
    InvalidResponse(String),
}

/// What a transport reports about a response.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Head {
    pub status: u16,
    pub content_type: Option<String>,
    pub content_length: Option<u64>,
    pub location: Option<String>,
}

/// One HTTP GET that never follows redirects. The body of a 2xx response is
/// written to `sink`, at most `max_bytes`; other bodies are discarded.
/// Implemented by [`HttpsTransport`] and by test fakes.
pub trait Transport: Send + Sync {
    fn get(
        &self,
        url: &Url,
        accept: &str,
        max_bytes: u64,
        sink: &mut dyn Write,
        cancel: &Cancel,
    ) -> Result<Head, NetError>;
}

// ---------------------------------------------------------------- addresses

pub fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || o[0] == 0
        || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64.0.0/10 carrier-grade NAT
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24 protocol assignments
        || (o[0] == 198 && (o[1] & 0xfe) == 18) // 198.18.0.0/15 benchmarking
        || o[0] >= 240) // reserved
}

pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_ipv4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_ipv4(v4);
            }
            let s = v6.segments();
            let nat64 = s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0];
            if nat64 {
                return is_public_ipv4(Ipv4Addr::new(
                    (s[6] >> 8) as u8,
                    s[6] as u8,
                    (s[7] >> 8) as u8,
                    s[7] as u8,
                ));
            }
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80 // link local
                || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
                || v6 == Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1))
        }
    }
}

/// Parse and check a URL Sentinel may request.
pub fn validate_url(raw: &str) -> Result<Url, NetError> {
    if raw.len() > MAX_URL_CHARS {
        return Err(NetError::InvalidUrl("the address is too long".into()));
    }
    if raw.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(NetError::InvalidUrl(
            "the address contains spaces or control characters".into(),
        ));
    }
    let url = Url::parse(raw).map_err(|e| NetError::InvalidUrl(e.to_string()))?;
    if url.scheme() != "https" {
        return Err(NetError::Blocked(format!(
            "only https addresses are used, not {}",
            url.scheme()
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(NetError::Blocked(
            "addresses with credentials are never used".into(),
        ));
    }
    if url.port().is_some_and(|p| p != 443) {
        return Err(NetError::Blocked(
            "only the standard HTTPS port is used".into(),
        ));
    }
    match url.host() {
        None => Err(NetError::InvalidUrl("the address has no host".into())),
        Some(url::Host::Ipv4(ip)) if !is_public_ipv4(ip) => {
            Err(NetError::Blocked(format!("{ip} is not a public address")))
        }
        Some(url::Host::Ipv6(ip)) if !is_public_ip(IpAddr::V6(ip)) => {
            Err(NetError::Blocked(format!("{ip} is not a public address")))
        }
        Some(url::Host::Ipv4(_) | url::Host::Ipv6(_)) => Ok(url),
        Some(url::Host::Domain(d)) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            let local = !d.contains('.')
                || [
                    "localhost",
                    ".localhost",
                    ".local",
                    ".internal",
                    ".lan",
                    ".home",
                    ".corp",
                    ".intranet",
                    ".home.arpa",
                ]
                .iter()
                .any(|s| d == s.trim_start_matches('.') || d.ends_with(s));
            if local {
                Err(NetError::Blocked(format!("{d} is a local name")))
            } else {
                Ok(url)
            }
        }
    }
}

// ------------------------------------------------------------------- robots

/// `(user agents, (allow, pattern) rules)` of one robots.txt group.
type RobotsGroup = (Vec<String>, Vec<(bool, String)>);

/// Parsed robots.txt rules that apply to Sentinel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Robots {
    /// `(allow, pattern)` rules.
    rules: Vec<(bool, String)>,
    disallow_all: bool,
}

impl Robots {
    pub fn allow_all() -> Self {
        Self::default()
    }
    pub fn disallow_all() -> Self {
        Self {
            rules: Vec::new(),
            disallow_all: true,
        }
    }

    /// The group for `sentinel` if there is one, else the `*` group.
    pub fn parse(text: &str) -> Self {
        let mut groups: Vec<RobotsGroup> = Vec::new();
        let mut agents_open = false;
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let (key, value) = (key.trim().to_ascii_lowercase(), value.trim().to_string());
            match key.as_str() {
                "user-agent" => {
                    if !agents_open {
                        groups.push((Vec::new(), Vec::new()));
                        agents_open = true;
                    }
                    groups
                        .last_mut()
                        .unwrap()
                        .0
                        .push(value.to_ascii_lowercase());
                }
                "allow" | "disallow" => {
                    agents_open = false;
                    if let Some(g) = groups.last_mut()
                        && !value.is_empty()
                    {
                        g.1.push((key == "allow", value));
                    }
                }
                _ => {}
            }
        }
        let pick = groups
            .iter()
            .find(|g| g.0.iter().any(|a| a.contains("sentinel")))
            .or_else(|| groups.iter().find(|g| g.0.iter().any(|a| a == "*")));
        Robots {
            rules: pick.map(|g| g.1.clone()).unwrap_or_default(),
            disallow_all: false,
        }
    }

    fn matches(pattern: &str, path: &str) -> Option<usize> {
        let anchored = pattern.ends_with('$');
        let pattern = pattern.trim_end_matches('$');
        let parts: Vec<&str> = pattern.split('*').collect();
        let mut pos = 0;
        for (i, part) in parts.iter().enumerate() {
            if i == 0 {
                if !path.starts_with(part) {
                    return None;
                }
                pos = part.len();
            } else {
                let found = path[pos..].find(part)?;
                pos += found + part.len();
            }
        }
        if anchored && pos != path.len() && !pattern.ends_with('*') {
            return None;
        }
        Some(pattern.len())
    }

    /// Longest matching rule wins; on a tie, allow wins.
    pub fn allows(&self, path_and_query: &str) -> bool {
        if self.disallow_all {
            return false;
        }
        let mut best: Option<(usize, bool)> = None;
        for (allow, pattern) in &self.rules {
            if let Some(len) = Self::matches(pattern, path_and_query)
                && best.is_none_or(|(l, a)| len > l || (len == l && *allow && !a))
            {
                best = Some((len, *allow));
            }
        }
        best.is_none_or(|(_, allow)| allow)
    }
}

// ------------------------------------------------------------------- client

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub timeout: Duration,
    pub min_interval: Duration,
}

/// A rate-limited, robots-respecting client over a [`Transport`].
pub struct Client {
    transport: Arc<dyn Transport>,
    limits: Limits,
    last: Mutex<HashMap<String, Instant>>,
    robots: Mutex<HashMap<String, Robots>>,
    requests: AtomicU32,
}

#[derive(Debug, Clone)]
pub struct Fetched {
    pub url: Url,
    pub head: Head,
}

impl Client {
    pub fn new(transport: Arc<dyn Transport>, limits: Limits) -> Self {
        Self {
            transport,
            limits,
            last: Mutex::new(HashMap::new()),
            robots: Mutex::new(HashMap::new()),
            requests: AtomicU32::new(0),
        }
    }

    pub fn requests(&self) -> u32 {
        self.requests.load(Ordering::SeqCst)
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Wait until this host may be contacted again; returns early on cancel.
    fn pace(&self, host: &str, cancel: &Cancel) -> Result<(), NetError> {
        loop {
            let wait = {
                let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
                let now = Instant::now();
                match last.get(host) {
                    Some(t) if now.duration_since(*t) < self.limits.min_interval => {
                        self.limits.min_interval - now.duration_since(*t)
                    }
                    _ => {
                        last.insert(host.to_string(), now);
                        return Ok(());
                    }
                }
            };
            if cancel.is_cancelled() {
                return Err(NetError::Cancelled);
            }
            std::thread::sleep(wait.min(Duration::from_millis(100)));
        }
    }

    fn raw(
        &self,
        url: &Url,
        accept: &str,
        max: u64,
        sink: &mut dyn Write,
        cancel: &Cancel,
    ) -> Result<Head, NetError> {
        if cancel.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        let host = url.host_str().unwrap_or_default().to_string();
        self.pace(&host, cancel)?;
        self.requests.fetch_add(1, Ordering::SeqCst);
        self.transport.get(url, accept, max, sink, cancel)
    }

    fn robots_allow(&self, url: &Url, cancel: &Cancel) -> Result<bool, NetError> {
        let host = url.host_str().unwrap_or_default().to_string();
        if let Some(r) = self
            .robots
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&host)
        {
            return Ok(r.allows(&path_and_query(url)));
        }
        let mut robots_url = url.clone();
        robots_url.set_path("/robots.txt");
        robots_url.set_query(None);
        robots_url.set_fragment(None);
        let mut body = Vec::new();
        let robots = match self.raw(
            &robots_url,
            "text/plain",
            MAX_ROBOTS_BYTES,
            &mut body,
            cancel,
        ) {
            Ok(h) if (200..300).contains(&h.status) => {
                Robots::parse(&String::from_utf8_lossy(&body))
            }
            // A redirect, 4xx or a missing file: no restrictions (RFC 9309).
            Ok(h) if (300..500).contains(&h.status) => Robots::allow_all(),
            // A server error or an oversized file: do not crawl (RFC 9309).
            Ok(_) | Err(NetError::Http(_) | NetError::TooLarge(_)) => Robots::disallow_all(),
            // Not reachable at all (offline, sockets blocked, timeout): nothing
            // is fetched either way; the real cause is reported, not "disallowed".
            Err(e) => return Err(e),
        };
        let allowed = robots.allows(&path_and_query(url));
        self.robots
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(host, robots);
        Ok(allowed)
    }

    /// GET `raw_url`, following validated redirects, honouring robots.txt
    /// and the rate limit. A 2xx body goes to `sink` (at most `max` bytes).
    pub fn fetch(
        &self,
        raw_url: &str,
        accept: &str,
        max: u64,
        sink: &mut dyn Write,
        cancel: &Cancel,
    ) -> Result<Fetched, NetError> {
        let mut url = validate_url(raw_url)?;
        for _ in 0..=MAX_REDIRECTS {
            if !self.robots_allow(&url, cancel)? {
                return Err(NetError::RobotsDisallowed);
            }
            // Only a 2xx body is written; the transport discards others.
            let head = self.raw(&url, accept, max, sink, cancel)?;
            match head.status {
                200..=299 => return Ok(Fetched { url, head }),
                301 | 302 | 303 | 307 | 308 => {
                    let location = head.location.clone().ok_or_else(|| {
                        NetError::InvalidResponse("redirect without a location".into())
                    })?;
                    let next = url
                        .join(&location)
                        .map_err(|e| NetError::InvalidUrl(e.to_string()))?;
                    url = validate_url(next.as_str())?;
                }
                429 => return Err(NetError::RateLimited),
                s => return Err(NetError::Http(s)),
            }
        }
        Err(NetError::TooManyRedirects)
    }

    /// Small bodies (API answers): fetched into memory, bounded.
    pub fn fetch_bytes(
        &self,
        raw_url: &str,
        accept: &str,
        max: u64,
        cancel: &Cancel,
    ) -> Result<(Vec<u8>, Fetched), NetError> {
        let mut body = Vec::new();
        let f = self.fetch(raw_url, accept, max, &mut body, cancel)?;
        Ok((body, f))
    }
}

pub fn path_and_query(url: &Url) -> String {
    match url.query() {
        Some(q) => format!("{}?{q}", url.path()),
        None => url.path().to_string(),
    }
}

/// Copy at most `max` bytes from `reader` to `sink`, checking `cancel`
/// between chunks. More than `max` bytes is an error, never a truncation.
pub fn copy_bounded(
    reader: &mut dyn Read,
    sink: &mut dyn Write,
    max: u64,
    cancel: &Cancel,
) -> Result<u64, NetError> {
    let mut buf = [0u8; 16 * 1024];
    let mut total: u64 = 0;
    loop {
        if cancel.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => return Ok(total),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => return Err(NetError::Timeout),
            Err(e) => return Err(NetError::Unreachable(e.to_string())),
        };
        total += n as u64;
        if total > max {
            return Err(NetError::TooLarge(max));
        }
        sink.write_all(&buf[..n])
            .map_err(|e| NetError::InvalidResponse(format!("could not store the response: {e}")))?;
    }
}

// --------------------------------------------------------------- real HTTPS

mod real {
    use super::*;
    use ureq::config::Config;
    use ureq::tls::{RootCerts, TlsConfig};
    use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
    use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

    /// Resolves with the system resolver and drops every non-public address.
    #[derive(Default)]
    pub struct PublicOnlyResolver(DefaultResolver);

    impl std::fmt::Debug for PublicOnlyResolver {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("PublicOnlyResolver")
        }
    }

    impl Resolver for PublicOnlyResolver {
        fn resolve(
            &self,
            uri: &ureq::http::Uri,
            config: &Config,
            timeout: NextTimeout,
        ) -> Result<ResolvedSocketAddrs, ureq::Error> {
            let found = self.0.resolve(uri, config, timeout)?;
            let mut out = self.empty();
            for addr in found.iter().filter(|a| is_public_ip(a.ip())) {
                out.push(*addr);
            }
            if out.is_empty() {
                return Err(ureq::Error::Other(Box::new(std::io::Error::other(
                    "the host resolves only to non-public addresses",
                ))));
            }
            Ok(out)
        }
    }

    pub struct HttpsTransport {
        agent: ureq::Agent,
    }

    impl HttpsTransport {
        pub fn new(timeout: Duration) -> Self {
            let config = Config::builder()
                .https_only(true)
                .max_redirects(0)
                .http_status_as_error(false)
                .proxy(None)
                .user_agent(USER_AGENT)
                .timeout_global(Some(timeout))
                .max_response_header_size(64 * 1024)
                .tls_config(
                    TlsConfig::builder()
                        .root_certs(RootCerts::PlatformVerifier)
                        .build(),
                )
                .build();
            Self {
                agent: ureq::Agent::with_parts(
                    config,
                    DefaultConnector::new(),
                    PublicOnlyResolver::default(),
                ),
            }
        }
    }

    fn map(e: ureq::Error) -> NetError {
        match e {
            ureq::Error::Timeout(_) => NetError::Timeout,
            ureq::Error::StatusCode(s) => NetError::Http(s),
            ureq::Error::Other(o) if o.to_string().contains("non-public") => {
                NetError::Blocked(o.to_string())
            }
            ureq::Error::HostNotFound => NetError::Unreachable("host not found (offline?)".into()),
            ureq::Error::BodyExceedsLimit(n) => NetError::TooLarge(n),
            ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => NetError::Timeout,
            other => NetError::Unreachable(other.to_string()),
        }
    }

    impl Transport for HttpsTransport {
        fn get(
            &self,
            url: &Url,
            accept: &str,
            max_bytes: u64,
            sink: &mut dyn Write,
            cancel: &Cancel,
        ) -> Result<Head, NetError> {
            let response = self
                .agent
                .get(url.as_str())
                .header("Accept", accept)
                .call()
                .map_err(map)?;
            let header = |name: &str| {
                response
                    .headers()
                    .get(name)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string)
            };
            let head = Head {
                status: response.status().as_u16(),
                content_type: header("content-type"),
                content_length: header("content-length").and_then(|v| v.parse().ok()),
                location: header("location"),
            };
            if !(200..300).contains(&head.status) {
                return Ok(head);
            }
            if head.content_length.is_some_and(|n| n > max_bytes) {
                return Err(NetError::TooLarge(max_bytes));
            }
            let mut reader = response
                .into_body()
                .into_with_config()
                .limit(max_bytes.saturating_add(1))
                .reader();
            copy_bounded(&mut reader, sink, max_bytes, cancel)?;
            Ok(head)
        }
    }
}

pub use real::HttpsTransport;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    type Route = Result<(Head, Vec<u8>), NetError>;

    /// A scripted transport: answers by URL, records every request.
    #[derive(Default)]
    pub struct Fake {
        pub routes: Mutex<HashMap<String, Route>>,
        pub log: Mutex<Vec<(String, Instant)>>,
    }

    impl Fake {
        pub fn ok(&self, url: &str, body: &[u8], content_type: &str) {
            self.routes.lock().unwrap().insert(
                url.into(),
                Ok((
                    Head {
                        status: 200,
                        content_type: Some(content_type.into()),
                        content_length: Some(body.len() as u64),
                        location: None,
                    },
                    body.to_vec(),
                )),
            );
        }
        pub fn status(&self, url: &str, status: u16, location: Option<&str>) {
            self.routes.lock().unwrap().insert(
                url.into(),
                Ok((
                    Head {
                        status,
                        content_type: None,
                        content_length: None,
                        location: location.map(str::to_string),
                    },
                    Vec::new(),
                )),
            );
        }
        pub fn fail(&self, url: &str, e: NetError) {
            self.routes.lock().unwrap().insert(url.into(), Err(e));
        }
        pub fn hits(&self, prefix: &str) -> usize {
            self.log
                .lock()
                .unwrap()
                .iter()
                .filter(|(u, _)| u.starts_with(prefix))
                .count()
        }
    }

    impl Transport for Fake {
        fn get(
            &self,
            url: &Url,
            _accept: &str,
            max: u64,
            sink: &mut dyn Write,
            cancel: &Cancel,
        ) -> Result<Head, NetError> {
            self.log
                .lock()
                .unwrap()
                .push((url.to_string(), Instant::now()));
            let route = self.routes.lock().unwrap().get(url.as_str()).cloned();
            match route {
                // Unknown robots.txt: none published.
                None if url.path() == "/robots.txt" => Ok(Head {
                    status: 404,
                    ..Default::default()
                }),
                None => Ok(Head {
                    status: 404,
                    ..Default::default()
                }),
                Some(Err(e)) => Err(e),
                Some(Ok((head, body))) => {
                    if (200..300).contains(&head.status) {
                        copy_bounded(&mut body.as_slice(), sink, max, cancel)?;
                    }
                    Ok(head)
                }
            }
        }
    }

    pub fn client(fake: Arc<Fake>) -> Client {
        Client::new(
            fake,
            Limits {
                timeout: Duration::from_secs(5),
                min_interval: Duration::from_millis(0),
            },
        )
    }

    #[test]
    fn urls_must_be_public_https() {
        assert!(validate_url("https://dl.dell.com/manuals/x.pdf").is_ok());
        for bad in [
            "http://dl.dell.com/x.pdf",
            "ftp://example.org/x",
            "file:///etc/passwd",
            "https://user:pw@example.org/",
            "https://example.org:8443/",
            "https://localhost/x",
            "https://printer.local/x",
            "https://intranet/x",
            "https://127.0.0.1/x",
            "https://10.0.0.5/x",
            "https://192.168.1.1/",
            "https://169.254.169.254/latest/meta-data/",
            "https://[::1]/",
            "https://[fe80::1]/",
            "https://[fd00::1]/",
            "https://[::ffff:127.0.0.1]/",
            "https://100.64.0.1/",
            "https://0.0.0.0/",
            "https://example.org/a b",
            "https://example.org/\n",
            "not a url",
        ] {
            assert!(validate_url(bad).is_err(), "{bad} accepted");
        }
        assert!(validate_url(&format!("https://example.org/{}", "a".repeat(3000))).is_err());
    }

    #[test]
    fn address_classes() {
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700::1111", "64:ff9b::808:808"] {
            assert!(is_public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.0.1",
            "169.254.1.1",
            "100.100.0.1",
            "198.18.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:10.0.0.1",
            "64:ff9b::7f00:1",
            "2001:db8::1",
        ] {
            assert!(!is_public_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn redirects_are_revalidated() {
        let fake = Arc::new(Fake::default());
        fake.status(
            "https://docs.example.org/a",
            302,
            Some("https://169.254.169.254/latest"),
        );
        let c = client(fake.clone());
        let err = c
            .fetch(
                "https://docs.example.org/a",
                "*/*",
                1000,
                &mut Vec::new(),
                &Cancel::new(),
            )
            .unwrap_err();
        assert!(matches!(err, NetError::Blocked(_)), "{err:?}");
        fake.status(
            "https://docs.example.org/b",
            301,
            Some("http://docs.example.org/b"),
        );
        assert!(matches!(
            c.fetch(
                "https://docs.example.org/b",
                "*/*",
                1000,
                &mut Vec::new(),
                &Cancel::new()
            ),
            Err(NetError::Blocked(_))
        ));
        fake.status("https://docs.example.org/c", 302, Some("/d"));
        fake.ok("https://docs.example.org/d", b"%PDF-1.4", "application/pdf");
        let f = c
            .fetch(
                "https://docs.example.org/c",
                "*/*",
                1000,
                &mut Vec::new(),
                &Cancel::new(),
            )
            .unwrap();
        assert_eq!(f.url.as_str(), "https://docs.example.org/d");
        fake.status("https://docs.example.org/loop", 302, Some("/loop"));
        assert_eq!(
            c.fetch(
                "https://docs.example.org/loop",
                "*/*",
                1000,
                &mut Vec::new(),
                &Cancel::new()
            )
            .unwrap_err(),
            NetError::TooManyRedirects
        );
    }

    #[test]
    fn robots_rules() {
        let r = Robots::parse(
            "User-agent: *\nDisallow: /private\nAllow: /private/public\nDisallow: /*.zip$\n\nUser-agent: OtherBot\nDisallow: /\n",
        );
        assert!(r.allows("/docs/x.pdf"));
        assert!(!r.allows("/private/x"));
        assert!(r.allows("/private/public/x"));
        assert!(!r.allows("/files/a.zip"));
        assert!(r.allows("/files/a.zip.html"));
        let s =
            Robots::parse("User-agent: *\nAllow: /\n\nUser-agent: Sentinel\nDisallow: /search\n");
        assert!(!s.allows("/search?q=x"), "a group naming Sentinel wins");
        assert!(Robots::parse("").allows("/anything"));
        assert!(!Robots::disallow_all().allows("/"));
    }

    #[test]
    fn robots_are_fetched_once_and_honoured() {
        let fake = Arc::new(Fake::default());
        fake.ok(
            "https://docs.example.org/robots.txt",
            b"User-agent: *\nDisallow: /nope\n",
            "text/plain",
        );
        fake.ok("https://docs.example.org/yes", b"ok", "text/plain");
        let c = client(fake.clone());
        assert!(
            c.fetch(
                "https://docs.example.org/yes",
                "*/*",
                100,
                &mut Vec::new(),
                &Cancel::new()
            )
            .is_ok()
        );
        assert_eq!(
            c.fetch(
                "https://docs.example.org/nope",
                "*/*",
                100,
                &mut Vec::new(),
                &Cancel::new()
            )
            .unwrap_err(),
            NetError::RobotsDisallowed
        );
        assert_eq!(fake.hits("https://docs.example.org/robots.txt"), 1);
        assert_eq!(
            fake.hits("https://docs.example.org/nope"),
            0,
            "a disallowed address is never requested"
        );
        // A robots.txt the server fails to deliver means: do not crawl.
        fake.status("https://busy.example.org/robots.txt", 503, None);
        assert_eq!(
            c.fetch(
                "https://busy.example.org/x",
                "*/*",
                100,
                &mut Vec::new(),
                &Cancel::new()
            )
            .unwrap_err(),
            NetError::RobotsDisallowed
        );
        // An unreachable host: the real cause, and the page is never requested.
        fake.fail(
            "https://down.example.org/robots.txt",
            NetError::Unreachable("sockets blocked".into()),
        );
        assert_eq!(
            c.fetch(
                "https://down.example.org/x",
                "*/*",
                100,
                &mut Vec::new(),
                &Cancel::new()
            )
            .unwrap_err(),
            NetError::Unreachable("sockets blocked".into())
        );
        assert_eq!(fake.hits("https://down.example.org/x"), 0);
    }

    #[test]
    fn rate_limit_spaces_requests_per_host() {
        let fake = Arc::new(Fake::default());
        fake.ok("https://docs.example.org/1", b"1", "text/plain");
        fake.ok("https://docs.example.org/2", b"2", "text/plain");
        let c = Client::new(
            fake.clone(),
            Limits {
                timeout: Duration::from_secs(5),
                min_interval: Duration::from_millis(150),
            },
        );
        let t0 = Instant::now();
        c.fetch(
            "https://docs.example.org/1",
            "*/*",
            10,
            &mut Vec::new(),
            &Cancel::new(),
        )
        .unwrap();
        c.fetch(
            "https://docs.example.org/2",
            "*/*",
            10,
            &mut Vec::new(),
            &Cancel::new(),
        )
        .unwrap();
        // robots, /1, /2: three requests to one host, at least 2 intervals.
        assert!(
            t0.elapsed() >= Duration::from_millis(300),
            "{:?}",
            t0.elapsed()
        );
        let log = fake.log.lock().unwrap();
        for w in log.windows(2) {
            assert!(w[1].1.duration_since(w[0].1) >= Duration::from_millis(140));
        }
    }

    #[test]
    fn http_errors_429_and_cancellation() {
        let fake = Arc::new(Fake::default());
        fake.status("https://docs.example.org/busy", 429, None);
        fake.status("https://docs.example.org/gone", 404, None);
        fake.fail("https://docs.example.org/slow", NetError::Timeout);
        let c = client(fake.clone());
        let none = Cancel::new();
        assert_eq!(
            c.fetch(
                "https://docs.example.org/busy",
                "*/*",
                10,
                &mut Vec::new(),
                &none
            )
            .unwrap_err(),
            NetError::RateLimited
        );
        assert_eq!(
            c.fetch(
                "https://docs.example.org/gone",
                "*/*",
                10,
                &mut Vec::new(),
                &none
            )
            .unwrap_err(),
            NetError::Http(404)
        );
        assert_eq!(
            c.fetch(
                "https://docs.example.org/slow",
                "*/*",
                10,
                &mut Vec::new(),
                &none
            )
            .unwrap_err(),
            NetError::Timeout
        );
        let cancel = Cancel::new();
        cancel.cancel();
        assert_eq!(
            c.fetch(
                "https://docs.example.org/x",
                "*/*",
                10,
                &mut Vec::new(),
                &cancel
            )
            .unwrap_err(),
            NetError::Cancelled
        );
    }

    #[test]
    fn bodies_are_bounded() {
        let mut sink = Vec::new();
        let data = vec![7u8; 100];
        assert_eq!(
            copy_bounded(&mut data.as_slice(), &mut sink, 99, &Cancel::new()).unwrap_err(),
            NetError::TooLarge(99)
        );
        let mut sink = Vec::new();
        assert_eq!(
            copy_bounded(&mut data.as_slice(), &mut sink, 100, &Cancel::new()).unwrap(),
            100
        );
    }

    /// Real network, not part of routine runs: `cargo test -p sentinel-docs -- --ignored live`.
    #[test]
    #[ignore = "contacts a public website"]
    fn live_https_and_public_only_resolver() {
        let t = HttpsTransport::new(Duration::from_secs(15));
        let c = Client::new(
            Arc::new(t),
            Limits {
                timeout: Duration::from_secs(15),
                min_interval: Duration::from_millis(1000),
            },
        );
        let (body, f) = c.fetch_bytes("https://archive.org/advancedsearch.php?q=identifier%3Aiapublicdomain&fl%5B%5D=identifier&rows=1&output=json", "application/json", 64 * 1024, &Cancel::new()).unwrap();
        assert_eq!(f.head.status, 200);
        assert!(String::from_utf8_lossy(&body).contains("response"));
    }
}
