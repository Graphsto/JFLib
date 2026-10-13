// client -- a blocking HTTP/1.1 client: one connection per request, timeouts, a size limit,
// an optional connector for https (TLS wraps the TCP connection)
//
//   Client::new().request("GET", &url, &headers, body) -> Result<Response, HttpError>
//   client.with_connector(Arc<dyn Fn(&Url, TcpStream) -> Result<Box<dyn Stream>, HttpError>>)

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;

use plat::net::{NetError, TcpStream};

use crate::http::{self, Body, Headers, ReadError, Reader, Response};
use crate::url::Url;
use crate::Stream;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpError {
    /// not an http(s) URL, or https without a connector
    Url(String),
    Net(NetError),
    /// the TLS layer refused (certificate, handshake)
    Tls(String),
    /// a malformed answer
    Bad,
    /// an answer over the size limit
    TooLarge,
}

impl core::fmt::Display for HttpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HttpError::Url(s) => write!(f, "invalid address: {}", s),
            HttpError::Net(e) => write!(f, "{}", e),
            HttpError::Tls(s) => write!(f, "TLS: {}", s),
            HttpError::Bad => f.write_str("malformed HTTP answer"),
            HttpError::TooLarge => f.write_str("answer too large"),
        }
    }
}

impl From<NetError> for HttpError {
    fn from(e: NetError) -> Self {
        HttpError::Net(e)
    }
}

pub type Connector = Arc<dyn Fn(&Url, TcpStream) -> Result<Box<dyn Stream>, HttpError> + Send + Sync>;

#[derive(Clone)]
pub struct Client {
    pub connect_ms: u32,
    /// the longest wait for any read or write
    pub timeout_ms: u32,
    pub max_body: usize,
    pub user_agent: String,
    connector: Option<Connector>,
}

impl Default for Client {
    fn default() -> Self {
        Client::new()
    }
}

impl Client {
    pub fn new() -> Client {
        Client { connect_ms: 5000, timeout_ms: 30_000, max_body: 64 << 20, user_agent: String::from("CraftLIB-netio"), connector: None }
    }

    /// the TLS layer for https URLs
    pub fn with_connector(mut self, c: Connector) -> Client {
        self.connector = Some(c);
        self
    }

    /// open the connection a request to `url` goes over
    pub fn connect(&self, url: &Url) -> Result<Box<dyn Stream>, HttpError> {
        let addrs = crate::dns::resolve(&url.host, url.port)?;
        let tcp = TcpStream::connect_any(&addrs, self.connect_ms)?;
        let _ = tcp.set_nodelay(true);
        tcp.set_read_timeout(Some(self.timeout_ms));
        tcp.set_write_timeout(Some(self.timeout_ms));
        if url.is_https() {
            let c = self.connector.as_ref().ok_or_else(|| HttpError::Url(String::from("https needs a TLS connector")))?;
            c(url, tcp)
        } else {
            Ok(Box::new(tcp))
        }
    }

    /// send one request and read the whole answer
    pub fn request(&self, method: &str, url: &Url, headers: &Headers, body: &[u8]) -> Result<Response, HttpError> {
        let s = self.connect(url)?;
        let mut h = headers.clone();
        if h.get("host").is_none() {
            h.set("Host", &url.host_header());
        }
        if h.get("user-agent").is_none() {
            h.set("User-Agent", &self.user_agent);
        }
        h.set("Connection", "close");
        if !http::write_request(&*s, method, &url.target, &h, body) {
            return Err(HttpError::Net(NetError::new(plat::net::NetErrorKind::Reset, 0)));
        }
        let mut rd = Reader::new(&*s);
        let (status, headers) = loop {
            let (start, headers) = rd.head().map_err(read_err)?;
            let mut p = start.splitn(3, ' ');
            let (Some(v), Some(code)) = (p.next(), p.next()) else { return Err(HttpError::Bad) };
            if !v.starts_with("HTTP/1.") {
                return Err(HttpError::Bad);
            }
            let status: u16 = code.parse().map_err(|_| HttpError::Bad)?;
            if (100..200).contains(&status) {
                continue; // interim answers (100 Continue)
            }
            break (status, headers);
        };
        let no_body = method == "HEAD" || status == 204 || status == 304;
        let body = if no_body { alloc::vec::Vec::new() } else { rd.body(&headers, self.max_body, true).map_err(read_err)? };
        s.shutdown();
        Ok(Response { status, headers, body: Body::Bytes(body) })
    }

    pub fn get(&self, url: &Url, headers: &Headers) -> Result<Response, HttpError> {
        self.request("GET", url, headers, &[])
    }
}

/// requests over one kept-alive connection (reconnected when the server closed it)
pub struct Session {
    client: Client,
    url: Url,
    conn: Option<Box<dyn Stream>>,
}

impl Session {
    pub fn new(client: Client, url: Url) -> Session {
        Session { client, url, conn: None }
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    fn once(&mut self, method: &str, target: &str, headers: &Headers, body: &[u8], max_body: usize) -> Result<(Response, bool), (HttpError, bool)> {
        let reused = self.conn.is_some();
        if self.conn.is_none() {
            self.conn = Some(self.client.connect(&self.url).map_err(|e| (e, false))?);
        }
        let s = self.conn.as_ref().ok_or((HttpError::Bad, false))?;
        let mut h = headers.clone();
        if h.get("host").is_none() {
            h.set("Host", &self.url.host_header());
        }
        if h.get("user-agent").is_none() {
            h.set("User-Agent", &self.client.user_agent);
        }
        h.set("Connection", "keep-alive");
        if !http::write_request(&**s, method, target, &h, body) {
            return Err((HttpError::Net(NetError::new(plat::net::NetErrorKind::Reset, 0)), reused));
        }
        let mut rd = Reader::new(&**s);
        let (status, headers) = loop {
            let (start, headers) = match rd.head() {
                Ok(x) => x,
                // a reused connection the server had closed: no answer at all
                Err(ReadError::Closed) => return Err((HttpError::Bad, reused)),
                Err(e) => return Err((read_err(e), false)),
            };
            let mut p = start.splitn(3, ' ');
            let (Some(v), Some(code)) = (p.next(), p.next()) else { return Err((HttpError::Bad, false)) };
            if !v.starts_with("HTTP/1.") {
                return Err((HttpError::Bad, false));
            }
            let status: u16 = code.parse().map_err(|_| (HttpError::Bad, false))?;
            if (100..200).contains(&status) {
                continue;
            }
            break (status, headers);
        };
        let no_body = method == "HEAD" || status == 204 || status == 304;
        let close = headers.has_token("connection", "close");
        let until_close = close && headers.get("content-length").is_none() && headers.get("transfer-encoding").is_none();
        let body = if no_body { alloc::vec::Vec::new() } else { rd.body(&headers, max_body, until_close).map_err(|e| (read_err(e), false))? };
        Ok((Response { status, headers, body: Body::Bytes(body) }, close))
    }

    /// one request (an answer of at most `max_body` bytes)
    pub fn request(&mut self, method: &str, target: &str, headers: &Headers, body: &[u8], max_body: usize) -> Result<Response, HttpError> {
        for attempt in 0..2 {
            match self.once(method, target, headers, body, max_body) {
                Ok((r, close)) => {
                    if close {
                        self.conn = None;
                    }
                    return Ok(r);
                }
                Err((e, retry)) => {
                    self.conn = None;
                    if !(retry && attempt == 0) {
                        return Err(e);
                    }
                }
            }
        }
        Err(HttpError::Bad)
    }
}

fn read_err(e: ReadError) -> HttpError {
    match e {
        ReadError::Net(n) => HttpError::Net(n),
        ReadError::BodyTooLarge | ReadError::HeadTooLarge => HttpError::TooLarge,
        ReadError::Closed | ReadError::Bad => HttpError::Bad,
    }
}

/// "Bearer <token>" headers plus JSON content type
pub fn json_headers(token: &str) -> Headers {
    let mut h = Headers::new();
    h.set("Accept", "application/json");
    h.set("Content-Type", "application/json");
    if !token.is_empty() {
        h.set("Authorization", &format!("Bearer {}", token));
    }
    h
}
