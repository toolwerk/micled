//! HTTPS zur Hue Bridge mit festem Zertifikat.
//!
//! Die Bridge hat ein selbst signiertes Zertifikat von Signify, das auf ihre
//! Bridge-ID lautet. Beim Koppeln merkt sich MicLED den SHA-256-Fingerabdruck
//! des Zertifikats und spricht danach nur mit genau diesem. Den Namen prüft
//! der Verifier nicht, die Signaturen im Handshake schon.
//!
//! Eine Anfrage je Verbindung (`Connection: close`), Antworten als JSON.

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::WebPkiSupportedAlgorithms;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};
use serde_json::Value;

const TIMEOUT: Duration = Duration::from_secs(3);
/// Höchstgröße einer Antwort (die Liste aller Lampen ist der größte Brocken)
const MAX_RESPONSE: usize = 4 * 1024 * 1024;

/// Meldung, wenn die Bridge ein anderes Zertifikat zeigt als beim Koppeln
pub const CERT_CHANGED: &str = "Zertifikat der Bridge hat sich geändert, bitte neu koppeln";

/// SHA-256 eines Zertifikats als Hex in Kleinbuchstaben
pub fn fingerprint(der: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, der).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// Prüft das End-Zertifikat gegen den erwarteten Fingerabdruck (ohne
/// Erwartung wird jedes angenommen) und merkt sich den gesehenen.
#[derive(Debug)]
struct Pinned {
    expected: Option<String>,
    seen: Mutex<Option<String>>,
    algs: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let fp = fingerprint(end_entity.as_ref());
        *self.seen.lock().unwrap() = Some(fp.clone());
        match &self.expected {
            Some(e) if !e.eq_ignore_ascii_case(&fp) => Err(rustls::Error::General(CERT_CHANGED.into())),
            _ => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

/// `host` oder `host:port` (ohne Port 443)
fn split_port(host: &str) -> (&str, u16) {
    match host.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => match p.parse() {
            Ok(port) => (h, port),
            Err(_) => (host, 443),
        },
        _ => (host, 443),
    }
}

/// Text der Anfrage
fn request_bytes(host: &str, method: &str, path: &str, key: Option<&str>, body: Option<&Value>) -> Vec<u8> {
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAccept: application/json\r\n");
    if let Some(k) = key {
        head.push_str(&format!("hue-application-key: {k}\r\n"));
    }
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    if !body.is_empty() || method != "GET" {
        head.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    let mut out = head.into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

/// Eine Anfrage an die Bridge `host` (ohne Angabe Port 443). Mit `fingerprint` nur an
/// genau dieses Zertifikat, ohne (Koppeln) an jedes. Liefert die Antwort und
/// den Fingerabdruck des gesehenen Zertifikats.
pub fn request(
    host: &str,
    fingerprint: Option<&str>,
    method: &str,
    path: &str,
    key: Option<&str>,
    body: Option<Value>,
) -> Result<(Value, String), String> {
    let unreachable = |e: &dyn std::fmt::Display| format!("Hue-Bridge nicht erreichbar: {e}");
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(Pinned {
        expected: fingerprint.map(str::to_string),
        seen: Mutex::new(None),
        algs: provider.signature_verification_algorithms,
    });
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| unreachable(&e))?
        .dangerous()
        .with_custom_certificate_verifier(verifier.clone())
        .with_no_client_auth();
    let (name, port) = split_port(host);
    let name = ServerName::try_from(name.to_string()).map_err(|_| format!("Ungültige Adresse der Bridge: {host}"))?;
    let conn = ClientConnection::new(Arc::new(config), name).map_err(|e| unreachable(&e))?;

    let addr = (split_port(host).0, port)
        .to_socket_addrs()
        .map_err(|e| unreachable(&e))?
        .next()
        .ok_or_else(|| unreachable(&"Adresse unbekannt"))?;
    let tcp = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| unreachable(&e))?;
    tcp.set_read_timeout(Some(TIMEOUT)).map_err(|e| unreachable(&e))?;
    tcp.set_write_timeout(Some(TIMEOUT)).map_err(|e| unreachable(&e))?;
    let mut tls = StreamOwned::new(conn, tcp);

    let cert_error = || {
        let seen = verifier.seen.lock().unwrap().clone();
        match (fingerprint, seen) {
            (Some(e), Some(s)) if !e.eq_ignore_ascii_case(&s) => Some(CERT_CHANGED.to_string()),
            _ => None,
        }
    };
    let req = request_bytes(host, method, path, key, body.as_ref());
    if let Err(e) = tls.write_all(&req).and_then(|()| tls.flush()) {
        return Err(cert_error().unwrap_or_else(|| unreachable(&e)));
    }
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match tls.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > MAX_RESPONSE {
                    return Err("Antwort der Hue-Bridge ist zu groß".into());
                }
            }
            // Die Bridge schließt ohne close_notify; ob die Antwort vollständig
            // ist, prüft parse_response
            Err(e) if e.kind() == ErrorKind::UnexpectedEof && !buf.is_empty() => break,
            Err(e) => return Err(cert_error().unwrap_or_else(|| unreachable(&e))),
        }
    }
    let seen = verifier.seen.lock().unwrap().clone().unwrap_or_default();
    parse_response(&buf).map(|v| (v, seen))
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Rumpf mit `Transfer-Encoding: chunked` zusammensetzen
fn dechunk(mut rest: &[u8]) -> Result<Vec<u8>, String> {
    let incomplete = || "Hue-Bridge: Antwort unvollständig".to_string();
    let mut out = Vec::new();
    loop {
        let end = find(rest, b"\r\n").ok_or_else(incomplete)?;
        let line = std::str::from_utf8(&rest[..end]).map_err(|_| incomplete())?;
        let size = usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16).map_err(|_| incomplete())?;
        rest = &rest[end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if size.checked_add(2).is_none_or(|need| rest.len() < need) {
            return Err(incomplete());
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
}

/// Fehlertexte aus `errors[].description` (CLIP v2) bzw. `[{"error": …}]` (v1)
fn error_text(v: &Value) -> Option<String> {
    let texts: Vec<&str> = match v {
        Value::Object(o) => o.get("errors")?.as_array()?.iter().filter_map(|e| e["description"].as_str()).collect(),
        Value::Array(a) => a.iter().filter_map(|e| e["error"]["description"].as_str()).collect(),
        _ => return None,
    };
    (!texts.is_empty()).then(|| texts.join(", "))
}

/// HTTP-Antwort zerlegen: Statuszeile, Rumpf (chunked oder mit
/// `Content-Length`), JSON. Status ab 400 wird ein Fehler.
pub fn parse_response(raw: &[u8]) -> Result<Value, String> {
    let incomplete = || "Hue-Bridge: Antwort unvollständig".to_string();
    let split = find(raw, b"\r\n\r\n").ok_or_else(incomplete)?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| "Hue-Bridge: Antwort unlesbar".to_string())?;
    let body = &raw[split + 4..];
    let mut lines = head.split("\r\n");
    let status: u16 = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| "Hue-Bridge: Antwort unlesbar".to_string())?;
    let mut chunked = false;
    let mut length = None;
    for l in lines {
        let Some((name, value)) = l.split_once(':') else { continue };
        let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
        if name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked") {
            chunked = true;
        } else if name == "content-length" {
            length = value.parse::<usize>().ok();
        }
    }
    let body = if chunked {
        dechunk(body)?
    } else if let Some(n) = length {
        if body.len() < n {
            return Err(incomplete());
        }
        body[..n].to_vec()
    } else {
        body.to_vec()
    };
    let json: Value = if body.iter().all(u8::is_ascii_whitespace) {
        Value::Null
    } else {
        serde_json::from_slice(&body).map_err(|_| "Hue-Bridge: Antwort ist kein JSON".to_string())?
    };
    if status >= 400 {
        return Err(match error_text(&json) {
            Some(t) => format!("Hue-Bridge meldet Fehler {status}: {t}"),
            None => format!("Hue-Bridge meldet Fehler {status}"),
        });
    }
    Ok(json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fingerprint_is_sha256_hex() {
        assert_eq!(fingerprint(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn port_is_optional() {
        assert_eq!(split_port("192.168.1.20"), ("192.168.1.20", 443));
        assert_eq!(split_port("127.0.0.1:8443"), ("127.0.0.1", 8443));
        assert_eq!(split_port("bridge.local:x"), ("bridge.local:x", 443));
        assert_eq!(split_port("fe80::1"), ("fe80::1", 443));
    }

    #[test]
    fn request_has_key_and_length() {
        let r = String::from_utf8(request_bytes("10.0.0.2", "PUT", "/clip/v2/resource/light/a", Some("k"), Some(&json!({"on": {"on": true}})))).unwrap();
        assert!(r.starts_with("PUT /clip/v2/resource/light/a HTTP/1.1\r\nHost: 10.0.0.2\r\nConnection: close\r\n"));
        assert!(r.contains("hue-application-key: k\r\n"));
        assert!(r.ends_with("Content-Length: 18\r\n\r\n{\"on\":{\"on\":true}}"));
        let g = String::from_utf8(request_bytes("h", "GET", "/x", None, None)).unwrap();
        assert!(!g.contains("hue-application-key") && !g.contains("Content-Length"));
        assert!(g.ends_with("\r\n\r\n"));
    }

    #[test]
    fn parses_content_length() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 13\r\n\r\n{\"data\": [1]}";
        assert_eq!(parse_response(raw).unwrap(), json!({"data": [1]}));
        let short = b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\n\r\n{\"data\": [1]}";
        assert!(parse_response(short).unwrap_err().contains("unvollständig"));
    }

    #[test]
    fn parses_chunked() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\n{\"a\":\r\n2;x=y\r\n1}\r\n0\r\n\r\n";
        assert_eq!(parse_response(raw).unwrap(), json!({"a": 1}));
        let cut = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\n{\"a\":\r\n";
        assert!(parse_response(cut).is_err());
        let huge = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nffffffffffffffff\r\nx\r\n";
        assert!(parse_response(huge).is_err(), "riesige Blocklänge ohne Überlauf");
    }

    #[test]
    fn parses_body_until_close_and_empty_body() {
        assert_eq!(parse_response(b"HTTP/1.1 200 OK\r\n\r\n[{\"x\":1}]").unwrap(), json!([{"x": 1}]));
        assert_eq!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").unwrap(), Value::Null);
    }

    #[test]
    fn errors_carry_descriptions() {
        let raw = b"HTTP/1.1 403 Forbidden\r\n\r\n{\"errors\":[{\"description\":\"unauthorized user\"}]}";
        assert_eq!(parse_response(raw).unwrap_err(), "Hue-Bridge meldet Fehler 403: unauthorized user");
        assert_eq!(parse_response(b"HTTP/1.1 503 x\r\n\r\n").unwrap_err(), "Hue-Bridge meldet Fehler 503");
        assert!(parse_response(b"HTTP/1.1 200 OK\r\n\r\nkein json").unwrap_err().contains("kein JSON"));
        assert!(parse_response(b"Quatsch").is_err());
    }
}
