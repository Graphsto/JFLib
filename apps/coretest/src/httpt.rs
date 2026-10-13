// httpt -- lib/netio: URLs, a loopback HTTP server and client (GET, POST bodies, keep-alive,
// pipelining, chunked streams, limits, malformed and smuggling requests)

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use netio::http::{Body, Headers, Request, Response};
use netio::url::{percent_decode, percent_encode, query_pairs};
use netio::{Client, Server, ServerConfig, Url};
use plat::net::{IpAddr, SocketAddr, TcpStream};

use crate::T;

fn handler(r: &Request) -> Response {
    match (r.method.as_str(), r.path()) {
        ("GET", "/hello") => Response::text(200, &format!("hello {}", r.query("name").unwrap_or_default())),
        ("POST", "/echo") => Response::bytes(200, "application/octet-stream", r.body.clone()),
        ("GET", "/stream") => {
            let mut i = 0u32;
            Response {
                status: 200,
                headers: Headers::new(),
                body: Body::Stream {
                    len: None,
                    next: Box::new(move |buf: &mut Vec<u8>| {
                        buf.extend(core::iter::repeat_n(b'a' + (i % 26) as u8, 1000));
                        i += 1;
                        i < 300
                    }),
                },
            }
        }
        ("GET", "/auth") => Response::text(200, r.bearer().unwrap_or("-")),
        _ => Response::text(404, "nope"),
    }
}

/// raw bytes in, raw answer out (until the server closes)
fn raw(addr: &SocketAddr, req: &[u8]) -> String {
    let Ok(s) = TcpStream::connect(addr, 2000) else { return String::new() };
    s.set_read_timeout(Some(3000));
    let _ = s.write_all(req);
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    while let Ok(n) = s.read(&mut buf) {
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn run(t: &mut T) {
    urls(t);
    let cfg = ServerConfig { threads: 4, max_body: 1 << 20, idle_ms: 2000, ..ServerConfig::default() };
    let srv = Server::start(&SocketAddr::new(IpAddr::LOCALHOST_V4, 0), cfg, Arc::new(handler));
    t.check("server start", srv.is_ok());
    let Ok(srv) = srv else { return };
    let addr = srv.local_addr();
    let base = format!("http://127.0.0.1:{}", addr.port);
    let c = Client::new();
    let get = |p: &str| Url::parse(&format!("{}{}", base, p)).map(|u| c.get(&u, &Headers::new()));
    let r = get("/hello?name=W%C3%BCrfel");
    t.check("GET", matches!(r, Some(Ok(ref x)) if x.status == 200 && x.body_bytes() == "hello Würfel".as_bytes()));
    let r = get("/missing");
    t.check("404", matches!(r, Some(Ok(ref x)) if x.status == 404));
    let body: Vec<u8> = (0..500_000u32).map(|i| (i * 7) as u8).collect();
    let u = Url::parse(&format!("{}/echo", base));
    let r = u.map(|u| c.request("POST", &u, &Headers::new(), &body));
    t.check("POST 500 KB echo", matches!(r, Some(Ok(ref x)) if x.status == 200 && x.body_bytes() == &body[..]));
    let big = alloc::vec![0u8; 2 << 20];
    let r = Url::parse(&format!("{}/echo", base)).map(|u| c.request("POST", &u, &Headers::new(), &big));
    t.check("413 over the limit", matches!(r, Some(Ok(ref x)) if x.status == 413) || matches!(r, Some(Err(_))));
    let r = get("/stream");
    t.check("chunked stream", matches!(r, Some(Ok(ref x)) if x.status == 200 && x.body_bytes().len() == 300_000 && x.body_bytes()[299_999] == b'a' + (299 % 26) as u8));
    let mut h = netio::client::json_headers("tok-123");
    h.set("X-Test", "1");
    let r = Url::parse(&format!("{}/auth", base)).map(|u| c.get(&u, &h));
    t.check("bearer token", matches!(r, Some(Ok(ref x)) if x.body_bytes() == b"tok-123"));
    // keep-alive and pipelining: two requests in one write, both answered in order
    let a = raw(&addr, b"GET /hello?name=a HTTP/1.1\r\nHost: x\r\n\r\nGET /hello?name=b HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    t.check("pipelined", a.matches("HTTP/1.1 200 OK").count() == 2 && a.find("hello a").is_some_and(|i| a.find("hello b").is_some_and(|j| j > i)));
    // HTTP/1.0 closes by default
    let a = raw(&addr, b"GET /hello HTTP/1.0\r\n\r\n");
    t.check("http/1.0", a.starts_with("HTTP/1.1 200") && a.contains("Connection: close"));
    // chunked request body
    let a = raw(&addr, b"POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n");
    t.check("chunked request", a.ends_with("hello world"));
    // smuggling and garbage
    let a = raw(&addr, b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n");
    t.check("CL + TE refused", a.starts_with("HTTP/1.1 400"));
    t.check("garbage", raw(&addr, b"\x16\x03\x01\x00\xa5hello\r\n\r\n").starts_with("HTTP/1.1 400"));
    t.check("bad version", raw(&addr, b"GET / HTTP/2.0\r\n\r\n").starts_with("HTTP/1.1 505"));
    let mut huge = Vec::from(&b"GET / HTTP/1.1\r\n"[..]);
    for i in 0..400 {
        huge.extend_from_slice(format!("X-H{}: {}\r\n", i, "v".repeat(60)).as_bytes());
    }
    huge.extend_from_slice(b"\r\n");
    t.check("431 head too large", raw(&addr, &huge).starts_with("HTTP/1.1 431"));
    t.check("100-continue", raw(&addr, b"POST /echo HTTP/1.1\r\nHost: x\r\nExpect: 100-continue\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").starts_with("HTTP/1.1 100 Continue"));
    // many clients at once
    let ok = Arc::new(core::sync::atomic::AtomicU32::new(0));
    let mut hs = Vec::new();
    for k in 0..8 {
        let (ok, base) = (ok.clone(), base.clone());
        hs.push(plat::thread_spawn(0, Box::new(move || {
            let c = Client::new();
            for i in 0..10 {
                if let Some(Ok(r)) = Url::parse(&format!("{}/hello?name={}-{}", base, k, i)).map(|u| c.get(&u, &Headers::new())) {
                    if r.body_bytes() == format!("hello {}-{}", k, i).as_bytes() {
                        ok.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                    }
                }
            }
        })));
    }
    for h in hs.into_iter().flatten() {
        h.join();
    }
    t.eq("80 parallel requests", ok.load(core::sync::atomic::Ordering::Relaxed), 80);
    let t0 = plat::time_us();
    srv.stop();
    t.check("stop is quick", plat::time_us() - t0 < 3_000_000);
    t.check("refused after stop", TcpStream::connect(&addr, 500).is_err());
}

fn urls(t: &mut T) {
    let u = Url::parse("HTTPS://Backup.Example:9443/api/jobs?x=1");
    t.check("url parse", u.as_ref().is_some_and(|u| u.is_https() && u.host == "backup.example" && u.port == 9443 && u.target == "/api/jobs?x=1"));
    t.eq("url default port", Url::parse("http://h").map(|u| (u.port, u.target)), Some((80, String::from("/"))));
    t.eq("url v6", Url::parse("http://[::1]:8080/x").map(|u| (u.host.clone(), u.host_header())), Some((String::from("::1"), String::from("[::1]:8080"))));
    t.check("url bad", Url::parse("ftp://x").is_none() && Url::parse("http://").is_none() && Url::parse("http://u:p@h/").is_none());
    t.eq("percent round trip", percent_decode(&percent_encode("a b/ü?&="), false), String::from("a b/ü?&="));
    t.eq("query pairs", query_pairs("a=1&b=x+y&c=%2F"), alloc::vec![(String::from("a"), String::from("1")), (String::from("b"), String::from("x y")), (String::from("c"), String::from("/"))]);
    t.eq("bad escape kept", percent_decode("100%", false), String::from("100%"));
}
