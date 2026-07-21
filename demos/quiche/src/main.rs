//! A real HTTP/3 GET from Rust compiled to `wasm32-unknown-emscripten`, running
//! on Node.js, using tokio-quiche and BoringSSL.
//!
//! tokio-quiche drives quiche's sans-I/O QUIC and H3 state machines over the
//! Emscripten Tokio `UdpSocket`, which reaches the internet through node:dgram.
//! Its default client configuration does not verify the certificate chain, so
//! this transport PoC also works behind a TLS-inspecting proxy. Not for production.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio_quiche::http3::driver::{ClientH3Event, H3Event, InboundFrame, NewClientRequest};
use tokio_quiche::quiche::h3::{Header, NameValue};
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

#[wasm_bindgen(tokio)]
pub async fn quiche_demo() -> Result<String, JsError> {
    run().await.map_err(|e| JsError::new(&format!("{e:#}")))
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
    let socket = UdpSocket::bind(bind_addr).await?;
    socket.connect(peer).await?;

    log(&format!("dialing QUIC {host} at {peer} (ALPN h3)..."));
    let (_connection, mut controller) = tokio::time::timeout(
        Duration::from_secs(30),
        tokio_quiche::quic::connect(socket, Some(host)),
    )
    .await
    .map_err(|_| anyhow::anyhow!("QUIC handshake timed out"))?
    .map_err(|error| anyhow::anyhow!("QUIC handshake failed: {error}"))?;
    log("QUIC handshake and HTTP/3 connection established");

    controller
        .request_sender()
        .send(NewClientRequest {
            request_id: 0,
            headers: vec![
                Header::new(b":method", b"GET"),
                Header::new(b":scheme", b"https"),
                Header::new(b":authority", host.as_bytes()),
                Header::new(b":path", b"/"),
                Header::new(b"user-agent", b"rust-workers-quic/0.1"),
            ],
            body_writer: None,
        })
        .map_err(|_| anyhow::anyhow!("HTTP/3 driver stopped before accepting request"))?;
    log(&format!("sent HTTP/3 GET https://{host}/"));

    let (status, server, body_len, preview) =
        tokio::time::timeout(Duration::from_secs(30), async {
            while let Some(event) = controller.event_receiver_mut().recv().await {
                match event {
                    ClientH3Event::Core(H3Event::IncomingHeaders(mut response)) => {
                        let mut status = 0;
                        let mut server = String::new();
                        for header in &response.headers {
                            match header.name() {
                                b":status" => {
                                    status = std::str::from_utf8(header.value())
                                        .ok()
                                        .and_then(|value| value.parse().ok())
                                        .unwrap_or(0);
                                }
                                b"server" => {
                                    server = String::from_utf8_lossy(header.value()).into_owned();
                                }
                                _ => {}
                            }
                        }

                        let mut body_len = 0;
                        let mut first_bytes = Vec::new();
                        while let Some(frame) = response.recv.recv().await {
                            if let InboundFrame::Body(data, fin) = frame {
                                body_len += data.len();
                                if first_bytes.len() < 120 {
                                    let take = (120 - first_bytes.len()).min(data.len());
                                    first_bytes.extend_from_slice(&data[..take]);
                                }
                                if fin {
                                    break;
                                }
                            }
                        }

                        let preview = String::from_utf8_lossy(&first_bytes)
                            .lines()
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        return Ok::<_, anyhow::Error>((status, server, body_len, preview));
                    }
                    ClientH3Event::Core(H3Event::ConnectionError(error)) => {
                        return Err(anyhow::anyhow!("HTTP/3 connection error: {error}"));
                    }
                    ClientH3Event::Core(H3Event::ConnectionShutdown(error)) => {
                        return Err(anyhow::anyhow!("HTTP/3 connection shut down: {error:?}"));
                    }
                    _ => {}
                }
            }
            Err(anyhow::anyhow!("HTTP/3 driver stopped before the response"))
        })
        .await
        .map_err(|_| anyhow::anyhow!("HTTP/3 response timed out"))??;

    if status == 0 {
        return Err(anyhow::anyhow!("response did not include an HTTP status"));
    }

    let output = format!(
        "HTTP/3 GET OK (tokio-quiche): {host} ({peer}) status={status} server={server:?} \
         body={body_len}B first_line={preview:?}"
    );
    log(&output);
    log("QUIC-H3-QUICHE-ON-EMSCRIPTEN-OK");
    Ok(output)
}
