//! Reading requests and writing responses on AirPlay's control connection.
//!
//! It is RTSP/1.0 with HTTP's framing: a request line, headers, and a body
//! as long as `Content-Length` says. Senders use `RTSP/1.0` for SETUP and
//! friends and sometimes `HTTP/1.1` for the POSTs; the answer uses whatever
//! the request used. Everything here is bounded, because anyone on the
//! network can open this connection.

use std::io;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAX_LINE: usize = 4096;
const MAX_HEADERS: usize = 32;
const MAX_BODY: usize = 1024 * 1024;

#[derive(Debug, Default)]
pub struct Request {
    pub method: String,
    pub uri: String,
    pub protocol: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The path without a query string.
    pub fn path(&self) -> &str {
        self.uri.split('?').next().unwrap_or("")
    }

    pub fn is_plist(&self) -> bool {
        self.header("Content-Type")
            .is_some_and(|t| t.contains("apple-binary-plist"))
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

async fn read_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    let read = (&mut *reader)
        .take(MAX_LINE as u64 + 1)
        .read_until(b'\n', &mut line)
        .await?;
    if read == 0 {
        return Ok(None);
    }
    if line.len() > MAX_LINE || line.last() != Some(&b'\n') {
        return Err(invalid("line too long"));
    }
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    String::from_utf8(line)
        .map(Some)
        .map_err(|_| invalid("line isn't UTF-8"))
}

/// The next request, or `None` when the sender closed the connection.
pub async fn read_request<R: AsyncBufRead + Unpin>(reader: &mut R) -> io::Result<Option<Request>> {
    // Some senders put an empty line between requests.
    let first = loop {
        match read_line(reader).await? {
            None => return Ok(None),
            Some(line) if line.is_empty() => continue,
            Some(line) => break line,
        }
    };
    let mut parts = first.split(' ');
    let (Some(method), Some(uri), Some(protocol)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(invalid("bad request line"));
    };
    let mut request = Request {
        method: method.to_owned(),
        uri: uri.to_owned(),
        protocol: protocol.to_owned(),
        ..Request::default()
    };
    loop {
        let line = read_line(reader)
            .await?
            .ok_or_else(|| invalid("closed in the headers"))?;
        if line.is_empty() {
            break;
        }
        if request.headers.len() == MAX_HEADERS {
            return Err(invalid("too many headers"));
        }
        let (name, value) = line.split_once(':').ok_or_else(|| invalid("bad header"))?;
        request
            .headers
            .push((name.trim().to_owned(), value.trim().to_owned()));
    }
    let length = match request.header("Content-Length") {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| invalid("bad Content-Length"))?,
        None => 0,
    };
    if length > MAX_BODY {
        return Err(invalid("body too large"));
    }
    request.body = vec![0; length];
    reader.read_exact(&mut request.body).await?;
    Ok(Some(request))
}

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub reason: &'static str,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
    /// Close the connection after this response.
    pub close: bool,
}

impl Response {
    pub fn ok() -> Self {
        Self {
            status: 200,
            reason: "OK",
            headers: Vec::new(),
            body: Vec::new(),
            close: false,
        }
    }

    pub fn status(status: u16, reason: &'static str) -> Self {
        Self {
            status,
            reason,
            ..Self::ok()
        }
    }

    pub fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }

    pub fn body(mut self, content_type: &'static str, body: Vec<u8>) -> Self {
        self.headers.push(("Content-Type", content_type.to_owned()));
        self.body = body;
        self
    }

    pub fn plist(self, value: &plist::Value) -> Self {
        let mut body = Vec::new();
        if let Err(error) = plist::to_writer_binary(&mut body, value) {
            tracing::warn!(%error, "writing a plist");
        }
        self.body("application/x-apple-binary-plist", body)
    }

    pub fn octets(self, body: Vec<u8>) -> Self {
        self.body("application/octet-stream", body)
    }

    pub fn closing(mut self) -> Self {
        self.close = true;
        self
    }

    pub fn to_bytes(&self, protocol: &str, cseq: Option<&str>) -> Vec<u8> {
        let protocol = if protocol.starts_with("HTTP/") {
            protocol
        } else {
            "RTSP/1.0"
        };
        let mut head = format!("{protocol} {} {}\r\n", self.status, self.reason);
        for (name, value) in &self.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("Server: AirTunes/220.68\r\n");
        if let Some(cseq) = cseq {
            head.push_str(&format!("CSeq: {cseq}\r\n"));
        }
        head.push_str(&format!("Content-Length: {}\r\n\r\n", self.body.len()));
        let mut bytes = head.into_bytes();
        bytes.extend_from_slice(&self.body);
        bytes
    }
}

pub async fn write_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    response: &Response,
    request: &Request,
) -> io::Result<()> {
    // CSeq is echoed back, so only a plain number is.
    let cseq = request.header("CSeq").filter(|value| {
        !value.is_empty() && value.len() <= 10 && value.bytes().all(|b| b.is_ascii_digit())
    });
    writer
        .write_all(&response.to_bytes(&request.protocol, cseq))
        .await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn reads_requests_back_to_back() {
        let raw = b"POST /pair-setup RTSP/1.0\r\nContent-Length: 3\r\nCSeq: 1\r\n\r\nabc\r\nGET /info RTSP/1.0\r\nCSeq: 2\r\n\r\n";
        let mut reader = BufReader::new(&raw[..]);
        let first = read_request(&mut reader).await.unwrap().unwrap();
        assert_eq!(first.method, "POST");
        assert_eq!(first.path(), "/pair-setup");
        assert_eq!(first.body, b"abc");
        assert_eq!(first.header("cseq"), Some("1"));
        let second = read_request(&mut reader).await.unwrap().unwrap();
        assert_eq!(second.uri, "/info");
        assert!(second.body.is_empty());
        assert!(read_request(&mut reader).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn refuses_oversized_input() {
        let raw = format!("GET / RTSP/1.0\r\nContent-Length: {}\r\n\r\n", MAX_BODY + 1);
        let mut reader = BufReader::new(raw.as_bytes());
        assert!(read_request(&mut reader).await.is_err());
        let long = format!("GET /{} RTSP/1.0\r\n\r\n", "a".repeat(MAX_LINE));
        let mut reader = BufReader::new(long.as_bytes());
        assert!(read_request(&mut reader).await.is_err());
    }

    #[test]
    fn response_echoes_the_protocol_and_cseq() {
        let bytes = Response::ok()
            .octets(vec![1, 2])
            .to_bytes("RTSP/1.0", Some("7"));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("RTSP/1.0 200 OK\r\n"));
        assert!(text.contains("CSeq: 7\r\n"));
        assert!(text.contains("Content-Length: 2\r\n\r\n"));
        let http = Response::ok().to_bytes("HTTP/1.1", None);
        assert!(String::from_utf8_lossy(&http).starts_with("HTTP/1.1 200 OK"));
    }
}
