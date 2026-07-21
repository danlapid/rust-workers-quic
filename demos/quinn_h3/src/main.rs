//! HTTP/3 over quinn from `wasm32-unknown-emscripten` running under Node.js.
//!
//! quinn-udp uses its normal Unix socket path over Emscripten's Node backend.

use std::net::SocketAddr;
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use wasm_bindgen::prelude::*;

fn main() {}

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
    println!("resolved {host} -> {addrs:?}");
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
    let mut endpoint = quinn::Endpoint::client(bind_addr.parse()?)?;
    endpoint.set_default_client_config(client_config()?);

    println!("dialing QUIC {host} at {peer} (ALPN h3)...");
    let connection = endpoint.connect(peer, host)?.await?;
    let alpn = connection
        .handshake_data()
        .and_then(|data| data.downcast::<quinn::crypto::rustls::HandshakeData>().ok())
        .and_then(|data| data.protocol)
        .map(|protocol| String::from_utf8_lossy(&protocol).into_owned())
        .unwrap_or_default();
    let rtt = connection.stats().path.rtt;
    println!("QUIC handshake OK: alpn={alpn}; rtt={rtt:?}");

    let (mut driver, mut requests) = h3::client::new(h3_quinn::Connection::new(connection))
        .await
        .map_err(|error| anyhow::anyhow!("initialize HTTP/3: {error}"))?;
    let drive = tokio::spawn(async move {
        let _ = std::future::poll_fn(|cx| driver.poll_close(cx)).await;
    });

    let request = http::Request::get(format!("https://{host}/"))
        .header("user-agent", "rust-workers-quic/0.1")
        .body(())?;
    println!("sending HTTP/3 GET https://{host}/ ...");
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
    println!("{output}");
    println!("QUIC-H3-ON-EMSCRIPTEN-OK");

    drop(requests);
    let _ = drive.await;
    endpoint.wait_idle().await;
    Ok(output)
}
