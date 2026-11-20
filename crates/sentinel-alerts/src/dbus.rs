//! A minimal D-Bus client for desktop notifications.
//!
//! It does one thing: call `org.freedesktop.Notifications.Notify` on the
//! user's session bus (a Unix socket). It connects, authenticates with
//! `EXTERNAL` (the kernel-verified user id), says `Hello`, sends the call
//! and waits for its reply. Messages are built by hand in little-endian
//! D-Bus wire format; no other method is callable through this module.
//! Notifications carry no actions: there is nothing to click that would do
//! anything to the system.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(3);
const MAX_MESSAGE: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub summary: String,
    pub body: String,
    /// 0 low, 1 normal, 2 critical (freedesktop urgency levels).
    pub urgency: u8,
}

/// Where the session bus is. Accepts `unix:path=...` and
/// `unix:abstract=...` entries of a D-Bus address.
pub fn parse_address(address: &str) -> Option<BusTarget> {
    for entry in address.split(';') {
        let Some(rest) = entry.strip_prefix("unix:") else {
            continue;
        };
        for kv in rest.split(',') {
            if let Some(path) = kv.strip_prefix("path=") {
                return Some(BusTarget::Path(unescape(path)));
            }
            if let Some(name) = kv.strip_prefix("abstract=") {
                return Some(BusTarget::Abstract(unescape(name)));
            }
        }
    }
    None
}

fn unescape(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(v) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusTarget {
    Path(String),
    Abstract(String),
}

fn connect(target: &BusTarget) -> std::io::Result<UnixStream> {
    match target {
        BusTarget::Path(p) => UnixStream::connect(p),
        BusTarget::Abstract(name) => {
            use std::os::linux::net::SocketAddrExt;
            let addr = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes())?;
            UnixStream::connect_addr(&addr)
        }
    }
}

// ------------------------------------------------------------ marshalling

struct Buf(Vec<u8>);

impl Buf {
    fn align(&mut self, n: usize) {
        while !self.0.len().is_multiple_of(n) {
            self.0.push(0);
        }
    }
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.align(4);
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.align(4);
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn string(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
        self.0.push(0);
    }
    fn signature(&mut self, s: &str) {
        self.u8(s.len() as u8);
        self.0.extend_from_slice(s.as_bytes());
        self.0.push(0);
    }
    /// Array: length placeholder, padding to the element alignment, then
    /// elements; the length excludes that padding.
    fn array(&mut self, element_align: usize, fill: impl FnOnce(&mut Buf)) {
        self.align(4);
        let at = self.0.len();
        self.0.extend_from_slice(&[0; 4]);
        self.align(element_align);
        let start = self.0.len();
        fill(self);
        let len = (self.0.len() - start) as u32;
        self.0[at..at + 4].copy_from_slice(&len.to_le_bytes());
    }
}

/// Text sent to the notification server: printable characters only, and
/// not too long. Notification servers may interpret a little markup, so
/// angle brackets and ampersands are escaped.
fn clean(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(max)
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn message(
    serial: u32,
    path: &str,
    interface: &str,
    member: &str,
    destination: &str,
    signature: Option<&str>,
    body: &[u8],
) -> Vec<u8> {
    let mut m = Buf(Vec::new());
    m.0.extend_from_slice(&[b'l', 1, 0, 1]);
    m.u32(body.len() as u32);
    m.u32(serial);
    m.array(8, |a| {
        let field = |a: &mut Buf, code: u8, sig: &str, write: &dyn Fn(&mut Buf)| {
            a.align(8);
            a.u8(code);
            a.signature(sig);
            write(a);
        };
        field(a, 1, "o", &|a| a.string(path));
        field(a, 2, "s", &|a| a.string(interface));
        field(a, 3, "s", &|a| a.string(member));
        field(a, 6, "s", &|a| a.string(destination));
        if let Some(sig) = signature {
            field(a, 8, "g", &|a| a.signature(sig));
        }
    });
    m.align(8);
    m.0.extend_from_slice(body);
    m.0
}

pub fn hello_message(serial: u32) -> Vec<u8> {
    message(
        serial,
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "Hello",
        "org.freedesktop.DBus",
        None,
        &[],
    )
}

pub fn notify_message(serial: u32, n: &Notification) -> Vec<u8> {
    notify_message_to(serial, n, "org.freedesktop.Notifications")
}

fn notify_message_to(serial: u32, n: &Notification, destination: &str) -> Vec<u8> {
    let mut b = Buf(Vec::new());
    b.string("Sentinel"); // app_name
    b.u32(0); // replaces_id
    b.string("dialog-information"); // app_icon
    b.string(&clean(&n.summary, 200));
    b.string(&clean(&n.body, 1000));
    b.array(4, |_| {}); // actions: none, ever
    b.array(8, |a| {
        // hints: {"urgency": <byte>}
        a.align(8);
        a.string("urgency");
        a.signature("y");
        a.u8(n.urgency.min(2));
    });
    b.i32(-1); // expire_timeout: server default
    message(
        serial,
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "Notify",
        destination,
        Some("susssasa{sv}i"),
        &b.0,
    )
}

// --------------------------------------------------------------- replies

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub kind: u8,
    pub reply_serial: Option<u32>,
    pub error_name: Option<String>,
}

/// Parse a message header far enough to find its type, reply serial and
/// error name. Returns the parsed reply and the total message length.
pub fn parse_reply(buf: &[u8]) -> Option<(Reply, usize)> {
    if buf.len() < 16 {
        return None;
    }
    let le = match buf[0] {
        b'l' => true,
        b'B' => false,
        _ => return None,
    };
    let u32_at = |i: usize| -> Option<u32> {
        let b: [u8; 4] = buf.get(i..i + 4)?.try_into().ok()?;
        Some(if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    let body_len = u32_at(4)? as usize;
    let fields_len = u32_at(12)? as usize;
    let header_end = 16 + fields_len;
    let total = header_end.div_ceil(8) * 8 + body_len;
    if total > MAX_MESSAGE {
        return None;
    }
    let mut reply = Reply {
        kind: buf[1],
        reply_serial: None,
        error_name: None,
    };
    let mut pos = 16;
    while pos < header_end {
        pos = pos.div_ceil(8) * 8;
        if pos >= header_end {
            break;
        }
        let code = *buf.get(pos)?;
        let sig_len = *buf.get(pos + 1)? as usize;
        let sig = buf.get(pos + 2..pos + 2 + sig_len)?;
        pos += 3 + sig_len;
        match sig {
            b"u" => {
                pos = pos.div_ceil(4) * 4;
                let v = u32_at(pos)?;
                if code == 5 {
                    reply.reply_serial = Some(v);
                }
                pos += 4;
            }
            b"s" | b"o" => {
                pos = pos.div_ceil(4) * 4;
                let len = u32_at(pos)? as usize;
                let text = buf.get(pos + 4..pos + 4 + len)?;
                if code == 4 {
                    reply.error_name = Some(String::from_utf8_lossy(text).into_owned());
                }
                pos += 4 + len + 1;
            }
            b"g" => {
                let len = *buf.get(pos)? as usize;
                pos += 2 + len;
            }
            _ => return None,
        }
    }
    Some((reply, total))
}

fn read_message(reader: &mut BufReader<UnixStream>) -> std::io::Result<Reply> {
    let mut head = [0u8; 16];
    reader.read_exact(&mut head)?;
    let fields_len = u32::from_le_bytes(head[12..16].try_into().unwrap_or([0; 4])) as usize;
    let body_len = u32::from_le_bytes(head[4..8].try_into().unwrap_or([0; 4])) as usize;
    let total = (16 + fields_len).div_ceil(8) * 8 + body_len;
    if head[0] != b'l' || total > MAX_MESSAGE {
        return Err(std::io::Error::other(
            "unsupported or oversized D-Bus message",
        ));
    }
    let mut buf = head.to_vec();
    buf.resize(total, 0);
    reader.read_exact(&mut buf[16..])?;
    parse_reply(&buf)
        .map(|(r, _)| r)
        .ok_or_else(|| std::io::Error::other("malformed D-Bus message"))
}

/// Send one notification. Blocks for at most a few seconds.
pub fn send(target: &BusTarget, uid: u32, n: &Notification) -> Result<(), String> {
    exchange(target, uid, n, "org.freedesktop.Notifications")
}

/// Self-test that shows nothing: send a well-formed `Notify` to a name no
/// service owns. The bus validates the whole message before routing, so an
/// `org.freedesktop.DBus.Error.ServiceUnknown` answer proves the connection,
/// authentication and message encoding work.
pub fn probe(target: &BusTarget, uid: u32) -> Result<(), String> {
    let n = Notification {
        summary: "probe".into(),
        body: String::new(),
        urgency: 0,
    };
    match exchange(target, uid, &n, "org.sentinel.NoSuchService.Probe") {
        Err(e) if e.contains("org.freedesktop.DBus.Error.ServiceUnknown") => Ok(()),
        Err(e) => Err(e),
        Ok(()) => Err("unexpected success".into()),
    }
}

fn exchange(
    target: &BusTarget,
    uid: u32,
    n: &Notification,
    destination: &str,
) -> Result<(), String> {
    let stream = connect(target).map_err(|e| format!("cannot reach the session bus: {e}"))?;
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);
    let hex: String = uid
        .to_string()
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect();
    reader
        .get_mut()
        .write_all(format!("\0AUTH EXTERNAL {hex}\r\n").as_bytes())
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("authentication failed: {e}"))?;
    if !line.starts_with("OK ") {
        return Err("the session bus rejected authentication".into());
    }
    reader
        .get_mut()
        .write_all(b"BEGIN\r\n")
        .map_err(|e| e.to_string())?;
    reader
        .get_mut()
        .write_all(&hello_message(1))
        .map_err(|e| e.to_string())?;
    reader
        .get_mut()
        .write_all(&notify_message_to(2, n, destination))
        .map_err(|e| e.to_string())?;
    // Skip the Hello reply and any signals until the answer to Notify.
    for _ in 0..16 {
        let reply = read_message(&mut reader)
            .map_err(|e| format!("no answer from the notification service: {e}"))?;
        if reply.reply_serial == Some(2) {
            return match reply.kind {
                2 => Ok(()),
                3 => Err(format!(
                    "the notification service refused: {}",
                    reply.error_name.unwrap_or_else(|| "unknown error".into())
                )),
                _ => Err("unexpected answer from the session bus".into()),
            };
        }
    }
    Err("no answer from the notification service".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        assert_eq!(
            parse_address("unix:path=/run/user/1000/bus"),
            Some(BusTarget::Path("/run/user/1000/bus".into()))
        );
        assert_eq!(
            parse_address("unix:path=/tmp/a%20b,guid=123"),
            Some(BusTarget::Path("/tmp/a b".into()))
        );
        assert_eq!(
            parse_address("tcp:host=x;unix:abstract=/tmp/dbus-1"),
            Some(BusTarget::Abstract("/tmp/dbus-1".into()))
        );
        assert_eq!(parse_address("tcp:host=localhost,port=1"), None);
    }

    #[test]
    fn notify_message_is_well_formed() {
        let msg = notify_message(
            7,
            &Notification {
                summary: "Sentinel — Low <b>battery</b>".into(),
                body: "Connect\u{7} the charger".into(),
                urgency: 2,
            },
        );
        // Header parses back, total length matches.
        let (parsed, total) = parse_reply(&msg).unwrap();
        assert_eq!(total, msg.len());
        assert_eq!(parsed.kind, 1);
        // The body length field matches the bytes after the 8-aligned header.
        let body_len = u32::from_le_bytes(msg[4..8].try_into().unwrap()) as usize;
        let fields_len = u32::from_le_bytes(msg[12..16].try_into().unwrap()) as usize;
        assert_eq!((16 + fields_len).div_ceil(8) * 8 + body_len, msg.len());
        let text = String::from_utf8_lossy(&msg);
        assert!(text.contains("susssasa{sv}i"));
        assert!(text.contains("&lt;b&gt;battery"), "markup is escaped");
        assert!(
            text.contains("Connect the charger"),
            "control characters removed"
        );
        assert!(text.contains("urgency"));
    }

    #[test]
    fn replies_are_parsed() {
        // A METHOD_RETURN with reply_serial = 2 and no body.
        let mut m = Buf(Vec::new());
        m.0.extend_from_slice(&[b'l', 2, 1, 1]);
        m.u32(0);
        m.u32(10);
        m.array(8, |a| {
            a.align(8);
            a.u8(5);
            a.signature("u");
            a.u32(2);
            a.align(8);
            a.u8(7);
            a.signature("s");
            a.string(":1.1");
        });
        m.align(8);
        let (r, total) = parse_reply(&m.0).unwrap();
        assert_eq!((r.kind, r.reply_serial), (2, Some(2)));
        assert_eq!(total, m.0.len());
        assert!(parse_reply(b"short").is_none());
    }
}
