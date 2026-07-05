//! THE POC: a real QUIC handshake to a public endpoint from Rust compiled to
//! `wasm32-unknown-emscripten`, running on Node.js.
//!
//! Stack:
//!   quinn (QUIC)  ->  our `AsyncUdpSocket` adapter  ->  tokio::net::UdpSocket
//!   (emscripten reactor)  ->  node:dgram  ->  the internet
//! TLS 1.3 via rustls + ring. No `quinn-udp` platform code is used: we pass an
//! abstract socket to `Endpoint::new_with_abstract_socket`.
use std::io::{self, IoSliceMut};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::io::ReadBuf;
use tokio::net::UdpSocket;
use wasm_bindgen::prelude::*;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};

fn main() {}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn console_log(s: &str);
}
fn log(s: &str) {
    console_log(s);
}

// ===== quinn AsyncUdpSocket over the emscripten tokio UdpSocket =====

#[derive(Debug)]
struct EmscriptenUdp {
    sock: UdpSocket,
}

impl quinn::AsyncUdpSocket for EmscriptenUdp {
    fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn quinn::UdpPoller>> {
        Box::pin(EmscriptenPoller { inner: self })
    }

    fn try_send(&self, transmit: &quinn::udp::Transmit) -> io::Result<()> {
        // Single datagram per send (no GSO); quinn handles WouldBlock via the poller.
        self.sock
            .try_send_to(transmit.contents, transmit.destination)
            .map(|_| ())
    }

    fn poll_recv(
        &self,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
        meta: &mut [quinn::udp::RecvMeta],
    ) -> Poll<io::Result<usize>> {
        if bufs.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let mut rb = ReadBuf::new(&mut bufs[0][..]);
        match self.sock.poll_recv_from(cx, &mut rb) {
            Poll::Ready(Ok(addr)) => {
                let len = rb.filled().len();
                meta[0] = quinn::udp::RecvMeta {
                    addr,
                    len,
                    stride: len,
                    ecn: None,
                    dst_ip: None,
                };
                Poll::Ready(Ok(1))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.sock.local_addr()
    }

    fn max_transmit_segments(&self) -> usize {
        1
    }

    fn max_receive_segments(&self) -> usize {
        1
    }

    fn may_fragment(&self) -> bool {
        false
    }
}

#[derive(Debug)]
struct EmscriptenPoller {
    inner: Arc<EmscriptenUdp>,
}

impl quinn::UdpPoller for EmscriptenPoller {
    fn poll_writable(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner.sock.poll_send_ready(cx)
    }
}

// ===== POC-only cert verifier =====
//
// Logs the presented chain and accepts it. This exists ONLY to prove the QUIC +
// TLS transport works end-to-end on emscripten/Node; signatures are still
// checked via the ring provider, but the trust-anchor path is not enforced.
// DO NOT ship this — production must use the webpki verifier (set STRICT_VERIFY,
// see below). The signature over the handshake (CertificateVerify, the server
// leaf's key) IS still checked by ring; only the trust-anchor path is accepted —
// which is what lets us demonstrate the transport behind a TLS-inspecting proxy
// (e.g. a Zero-Trust VPN) whose private CA a public root store won't chain to.
#[derive(Debug)]
struct LoggingAcceptVerifier {
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl ServerCertVerifier for LoggingAcceptVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        console_log(&format!(
            "[cert] server_name={server_name:?} leaf={} bytes, {} intermediate(s) — accepting (POC)",
            end_entity.len(),
            intermediates.len(),
        ));
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

// ===== the handshake =====

// Set true for a clean network path (no TLS-inspecting proxy): full webpki
// verification against the Mozilla roots then validates the real server cert.
// Leave false if you're behind a Zero-Trust/VPN proxy that re-signs with a
// private CA (a public root store can't chain to it, and if that CA uses a key
// `ring` doesn't implement — e.g. ECDSA P-521 — strict verification can't
// succeed regardless). The accept verifier still ring-checks the leaf signature.
const STRICT_VERIFY: bool = false;

fn client_config() -> anyhow::Result<quinn::ClientConfig> {
    // ring as the crypto provider (aws-lc-rs is intractable on emscripten).
    let _ = rustls::crypto::ring::default_provider().install_default();
    let provider = Arc::new(rustls::crypto::ring::default_provider());

    let builder = rustls::ClientConfig::builder();
    let mut tls = if STRICT_VERIFY {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        log(&format!("STRICT verification: {} Mozilla trust anchors", roots.len()));
        builder.with_root_certificates(roots).with_no_client_auth()
    } else {
        log("POC verifier: ring-checks the leaf signature, accepts the chain path");
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(LoggingAcceptVerifier {
                provider: provider.clone(),
            }))
            .with_no_client_auth()
    };
    // HTTP/3 ALPN — cloudflare-quic.com speaks h3.
    tls.alpn_protocols = vec![b"h3".to_vec()];

    let qcc = quinn::crypto::rustls::QuicClientConfig::try_from(tls)
        .map_err(|e| anyhow::anyhow!("quic client config: {e}"))?;
    Ok(quinn::ClientConfig::new(Arc::new(qcc)))
}

#[wasm_bindgen(tokio)]
pub async fn quic_demo() -> Result<String, JsError> {
    run().await.map_err(|e| JsError::new(&format!("{e:#}")))
}

async fn run() -> anyhow::Result<String> {
    // Sanity-check the wasm clock — cert validity (and thus path building) needs
    // a correct time; a 1970 clock can surface as UnknownIssuer.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    log(&format!("wasm unix time = {now}"));

    let host = "cloudflare-quic.com";
    let port = 443u16;

    // Resolve via tokio async DNS (emscripten_dns_lookup_async). Prefer an IPv4
    // address (the node:dgram udp4 path we've validated) and bind the local UDP
    // socket to the matching family — quinn rejects a v6 peer on a v4 socket.
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port)).await?.collect();
    log(&format!("resolved {host} -> {addrs:?}"));
    let addr = addrs
        .iter()
        .find(|a| a.is_ipv4())
        .copied()
        .or_else(|| addrs.first().copied())
        .ok_or_else(|| anyhow::anyhow!("no address for {host}"))?;

    // Bind our reactor-backed UDP socket (matching family) and wrap it for quinn.
    let bind_addr = if addr.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
    let sock = UdpSocket::bind(bind_addr).await?;
    log(&format!("bound udp socket at {}", sock.local_addr()?));
    let socket: Arc<dyn quinn::AsyncUdpSocket> = Arc::new(EmscriptenUdp { sock });

    let mut endpoint = quinn::Endpoint::new_with_abstract_socket(
        quinn::EndpointConfig::default(),
        None,
        socket,
        Arc::new(quinn::TokioRuntime),
    )?;
    endpoint.set_default_client_config(client_config()?);

    log(&format!("dialing QUIC {host} at {addr} (ALPN h3)..."));
    let connecting = endpoint.connect(addr, host)?;
    let conn = connecting.await?;

    let alpn = conn
        .handshake_data()
        .and_then(|d| d.downcast::<quinn::crypto::rustls::HandshakeData>().ok())
        .and_then(|d| d.protocol.clone())
        .map(|p| String::from_utf8_lossy(&p).into_owned())
        .unwrap_or_default();
    let rtt = conn.stats().path.rtt;
    log(&format!("QUIC handshake OK: alpn={alpn}; rtt={rtt:?}"));

    // ===== HTTP/3 over the QUIC connection =====
    let h3_conn = h3_quinn::Connection::new(conn);
    let (mut driver, mut send_request) = h3::client::new(h3_conn)
        .await
        .map_err(|e| anyhow::anyhow!("h3 client init: {e}"))?;

    // The connection driver must be polled for the request to make progress.
    let drive = tokio::spawn(async move {
        let _ = std::future::poll_fn(|cx| driver.poll_close(cx)).await;
    });

    let req = http::Request::builder()
        .method("GET")
        .uri(format!("https://{host}/"))
        .header("user-agent", "rust-workers-quic/0.1")
        .body(())
        .map_err(|e| anyhow::anyhow!("build request: {e}"))?;

    log(&format!("sending HTTP/3 GET https://{host}/ ..."));
    let mut stream = send_request
        .send_request(req)
        .await
        .map_err(|e| anyhow::anyhow!("send_request: {e}"))?;
    stream
        .finish()
        .await
        .map_err(|e| anyhow::anyhow!("finish request: {e}"))?;

    let resp = stream
        .recv_response()
        .await
        .map_err(|e| anyhow::anyhow!("recv_response: {e}"))?;
    let status = resp.status();
    let server = resp
        .headers()
        .get("server")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    log(&format!("HTTP/3 response: status={status} server={server:?}"));

    // Drain the body, keeping the first line for a visible sanity check.
    let mut body_len = 0usize;
    let mut first_bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = stream
        .recv_data()
        .await
        .map_err(|e| anyhow::anyhow!("recv_data: {e}"))?
    {
        let bytes = bytes::Buf::chunk(&chunk);
        body_len += bytes.len();
        if first_bytes.len() < 120 {
            first_bytes.extend_from_slice(&bytes[..bytes.len().min(120 - first_bytes.len())]);
        }
    }
    let preview = String::from_utf8_lossy(&first_bytes);
    let preview = preview.lines().next().unwrap_or("").trim().to_string();

    let out = format!(
        "HTTP/3 GET OK: {host} ({addr}) alpn={alpn} rtt={rtt:?} status={status} \
         body={body_len}B first_line={preview:?}"
    );
    log(&out);
    log("QUIC-H3-ON-EMSCRIPTEN-OK");

    drop(send_request);
    let _ = drive.await;
    endpoint.wait_idle().await;
    Ok(out)
}
