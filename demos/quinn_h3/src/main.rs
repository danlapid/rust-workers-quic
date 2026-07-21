//! HTTP/3 over quinn from `wasm32-unknown-emscripten` running on Node.js.
//!
//! Quinn uses this small adapter instead of quinn-udp, putting its datagrams on
//! the Emscripten Tokio `UdpSocket` backed by node:dgram.

use std::io::{self, IoSliceMut};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use tokio::io::ReadBuf;
use tokio::net::UdpSocket;
use wasm_bindgen::prelude::*;

fn main() {}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn console_log(s: &str);
}

fn log(s: &str) {
    console_log(s);
}

#[derive(Debug)]
struct EmscriptenUdp {
    socket: UdpSocket,
}

impl quinn::AsyncUdpSocket for EmscriptenUdp {
    fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn quinn::UdpPoller>> {
        Box::pin(EmscriptenPoller(self))
    }

    fn try_send(&self, transmit: &quinn::udp::Transmit) -> io::Result<()> {
        self.socket
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

        let mut buf = ReadBuf::new(&mut bufs[0]);
        let addr = std::task::ready!(self.socket.poll_recv_from(cx, &mut buf))?;
        let len = buf.filled().len();
        meta[0] = quinn::udp::RecvMeta {
            addr,
            len,
            stride: len,
            ecn: None,
            dst_ip: None,
        };
        Poll::Ready(Ok(1))
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    fn may_fragment(&self) -> bool {
        false
    }
}

#[derive(Debug)]
struct EmscriptenPoller(Arc<EmscriptenUdp>);

impl quinn::UdpPoller for EmscriptenPoller {
    fn poll_writable(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.0.socket.poll_send_ready(cx)
    }
}

// The PoC must also work behind TLS-inspecting proxies. Accept the chain path,
// but still use ring to verify the server's TLS handshake signature.
#[derive(Debug)]
struct AcceptCertificate(rustls::crypto::WebPkiSupportedAlgorithms);

impl ServerCertVerifier for AcceptCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, signature, &self.0)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, signature, &self.0)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}

fn client_config() -> anyhow::Result<quinn::ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(AcceptCertificate(
        provider.signature_verification_algorithms,
    ));
    let mut tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| anyhow::anyhow!("TLS protocol configuration: {error}"))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"h3".to_vec()];

    let config = quinn::crypto::rustls::QuicClientConfig::try_from(tls)
        .map_err(|error| anyhow::anyhow!("QUIC client configuration: {error}"))?;
    Ok(quinn::ClientConfig::new(Arc::new(config)))
}

#[wasm_bindgen(tokio)]
pub async fn quic_demo() -> Result<String, JsError> {
    run()
        .await
        .map_err(|error| JsError::new(&format!("{error:#}")))
}

async fn run() -> anyhow::Result<String> {
    let host = "cloudflare-quic.com";
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, 443)).await?.collect();
    log(&format!("resolved {host} -> {addrs:?}"));
    let peer = addrs
        .iter()
        .find(|addr| addr.is_ipv4())
        .copied()
        .or_else(|| addrs.first().copied())
        .ok_or_else(|| anyhow::anyhow!("no address for {host}"))?;

    let bind_addr = if peer.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let socket: Arc<dyn quinn::AsyncUdpSocket> = Arc::new(EmscriptenUdp {
        socket: UdpSocket::bind(bind_addr).await?,
    });
    let mut endpoint = quinn::Endpoint::new_with_abstract_socket(
        quinn::EndpointConfig::default(),
        None,
        socket,
        Arc::new(quinn::TokioRuntime),
    )?;
    endpoint.set_default_client_config(client_config()?);

    log(&format!("dialing QUIC {host} at {peer} (ALPN h3)..."));
    let connection = endpoint.connect(peer, host)?.await?;
    let alpn = connection
        .handshake_data()
        .and_then(|data| data.downcast::<quinn::crypto::rustls::HandshakeData>().ok())
        .and_then(|data| data.protocol)
        .map(|protocol| String::from_utf8_lossy(&protocol).into_owned())
        .unwrap_or_default();
    let rtt = connection.stats().path.rtt;
    log(&format!("QUIC handshake OK: alpn={alpn}; rtt={rtt:?}"));

    let (mut driver, mut requests) = h3::client::new(h3_quinn::Connection::new(connection))
        .await
        .map_err(|error| anyhow::anyhow!("initialize HTTP/3: {error}"))?;
    let drive = tokio::spawn(async move {
        let _ = std::future::poll_fn(|cx| driver.poll_close(cx)).await;
    });

    let request = http::Request::get(format!("https://{host}/"))
        .header("user-agent", "rust-workers-quic/0.1")
        .body(())?;
    log(&format!("sending HTTP/3 GET https://{host}/ ..."));
    let mut stream = requests
        .send_request(request)
        .await
        .map_err(|error| anyhow::anyhow!("send request: {error}"))?;
    stream
        .finish()
        .await
        .map_err(|error| anyhow::anyhow!("finish request: {error}"))?;
    let status = stream
        .recv_response()
        .await
        .map_err(|error| anyhow::anyhow!("receive response: {error}"))?
        .status();

    let mut body = Vec::new();
    while let Some(chunk) = stream
        .recv_data()
        .await
        .map_err(|error| anyhow::anyhow!("receive body: {error}"))?
    {
        body.extend_from_slice(bytes::Buf::chunk(&chunk));
    }
    let preview = String::from_utf8_lossy(&body)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();

    let output = format!(
        "HTTP/3 GET OK: {host} ({peer}) alpn={alpn} rtt={rtt:?} status={status} \
         body={}B first_line={preview:?}",
        body.len()
    );
    log(&output);
    log("QUIC-H3-ON-EMSCRIPTEN-OK");

    drop(requests);
    let _ = drive.await;
    endpoint.wait_idle().await;
    Ok(output)
}
