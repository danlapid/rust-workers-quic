//! THE POC (quiche edition): a real QUIC + HTTP/3 GET to a public endpoint from
//! Rust compiled to `wasm32-unknown-emscripten`, running on Node.js — using
//! Cloudflare's own **quiche** (QUIC/H3) on **BoringSSL** via the `boring` crate.
//!
//! Stack:
//!   quiche (QUIC/H3, sans-I/O)  ->  our send()/recv() loop  ->  tokio::net::UdpSocket
//!   (emscripten reactor)  ->  node:dgram  ->  the internet
//! TLS 1.3 via BoringSSL (compiled to wasm, OPENSSL_NO_ASM). quiche is sans-I/O,
//! so no platform UDP code exists to port: we own the datagram loop.
//!
//! Cert verification is disabled (`verify_peer(false)`) for the PoC — it proves
//! the QUIC/TLS transport + H3 work on emscripten/Node and stays demonstrable
//! behind a TLS-inspecting proxy. NOT for production.
use std::net::SocketAddr;
use std::time::Duration;

use quiche::h3::{Event, Header, NameValue};
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

const MAX_DATAGRAM: usize = 1350;

/// A 16-byte source connection ID. RFC 9000 wants unpredictability; we seed
/// splitmix64 from the wasm clock (good enough for a client-side PoC — no `ring`
/// dep needed here since BoringSSL/quiche don't expose an RNG to us directly).
fn gen_scid() -> [u8; 16] {
    let mut s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9e37_79b9_7f4a_7c15);
    let mut o = [0u8; 16];
    for chunk in o.chunks_mut(8) {
        s = s.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = s;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        let n = chunk.len();
        chunk.copy_from_slice(&z.to_le_bytes()[..n]);
    }
    o
}

fn build_config() -> anyhow::Result<quiche::Config> {
    let mut c = quiche::Config::new(quiche::PROTOCOL_VERSION)
        .map_err(|e| anyhow::anyhow!("quiche::Config::new: {e}"))?;
    c.set_application_protos(&[b"h3"])
        .map_err(|e| anyhow::anyhow!("set_application_protos: {e}"))?;
    // PoC: skip cert-chain verification (BoringSSL still runs the TLS 1.3
    // handshake + key schedule). See the module doc.
    c.verify_peer(false);
    c.set_max_idle_timeout(15_000);
    c.set_max_recv_udp_payload_size(MAX_DATAGRAM);
    c.set_max_send_udp_payload_size(MAX_DATAGRAM);
    c.set_initial_max_data(10_000_000);
    c.set_initial_max_stream_data_bidi_local(2_000_000);
    c.set_initial_max_stream_data_bidi_remote(2_000_000);
    c.set_initial_max_stream_data_uni(2_000_000);
    c.set_initial_max_streams_bidi(100);
    c.set_initial_max_streams_uni(100);
    c.set_disable_active_migration(true);
    Ok(c)
}

/// Drain quiche's egress queue to the socket (single datagram per send; no GSO).
async fn flush(
    conn: &mut quiche::Connection,
    sock: &UdpSocket,
    out: &mut [u8],
) -> anyhow::Result<()> {
    loop {
        match conn.send(out) {
            Ok((write, info)) => {
                sock.send_to(&out[..write], info.to).await?;
            }
            Err(quiche::Error::Done) => break,
            Err(e) => return Err(anyhow::anyhow!("conn.send: {e}")),
        }
    }
    Ok(())
}

#[wasm_bindgen(tokio)]
pub async fn quiche_demo() -> Result<String, JsError> {
    run().await.map_err(|e| JsError::new(&format!("{e:#}")))
}

async fn run() -> anyhow::Result<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    log(&format!("wasm unix time = {now}"));

    let host = "cloudflare-quic.com";
    let port = 443u16;

    // tokio async DNS (emscripten_dns_lookup_async); prefer the validated v4 path.
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port)).await?.collect();
    log(&format!("resolved {host} -> {addrs:?}"));
    let peer = addrs
        .iter()
        .find(|a| a.is_ipv4())
        .copied()
        .or_else(|| addrs.first().copied())
        .ok_or_else(|| anyhow::anyhow!("no address for {host}"))?;

    let bind_addr = if peer.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
    let sock = UdpSocket::bind(bind_addr).await?;
    let local = sock.local_addr()?;
    log(&format!("bound udp socket at {local}"));

    let mut config = build_config()?;
    let scid_bytes = gen_scid();
    let scid = quiche::ConnectionId::from_ref(&scid_bytes);

    log(&format!("dialing QUIC {host} at {peer} (ALPN h3)..."));
    let mut conn = quiche::connect(Some(host), &scid, local, peer, &mut config)
        .map_err(|e| anyhow::anyhow!("quiche::connect: {e}"))?;

    let mut out = [0u8; MAX_DATAGRAM];
    let mut buf = [0u8; 65535];

    let mut h3: Option<quiche::h3::Connection> = None;
    let mut req_sent = false;
    let mut logged_established = false;

    let mut status = 0u16;
    let mut server = String::new();
    let mut body_len = 0usize;
    let mut first_bytes: Vec<u8> = Vec::new();
    let mut done = false;

    // Send the QUIC Initial.
    flush(&mut conn, &sock, &mut out).await?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);

    loop {
        if conn.is_closed() {
            log("connection closed");
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            log("deadline reached; stopping");
            break;
        }

        // Wait for ingress, bounded by quiche's own timeout.
        let timeout = conn.timeout().unwrap_or(Duration::from_millis(200));
        let recvd = tokio::select! {
            r = sock.recv_from(&mut buf) => Some(r?),
            _ = tokio::time::sleep(timeout) => None,
        };

        match recvd {
            Some((len, from)) => {
                let info = quiche::RecvInfo { from, to: local };
                if let Err(e) = conn.recv(&mut buf[..len], info) {
                    log(&format!("conn.recv: {e}"));
                }
            }
            None => conn.on_timeout(),
        }

        if conn.is_established() && !logged_established {
            let alpn = String::from_utf8_lossy(conn.application_proto()).into_owned();
            let rtt = conn.path_stats().next().map(|p| p.rtt).unwrap_or_default();
            log(&format!("QUIC handshake OK: alpn={alpn:?}; rtt={rtt:?}"));
            logged_established = true;
        }

        // Bring up HTTP/3 once the QUIC handshake completes.
        if conn.is_established() && h3.is_none() {
            let h3_config = quiche::h3::Config::new()
                .map_err(|e| anyhow::anyhow!("h3::Config::new: {e}"))?;
            match quiche::h3::Connection::with_transport(&mut conn, &h3_config) {
                Ok(c) => {
                    h3 = Some(c);
                    log("HTTP/3 connection established");
                }
                Err(e) => log(&format!("h3 with_transport (will retry): {e}")),
            }
        }

        if let Some(h3c) = h3.as_mut() {
            if !req_sent {
                let req = [
                    Header::new(b":method", b"GET"),
                    Header::new(b":scheme", b"https"),
                    Header::new(b":authority", host.as_bytes()),
                    Header::new(b":path", b"/"),
                    Header::new(b"user-agent", b"rust-workers-quic/0.1"),
                ];
                match h3c.send_request(&mut conn, &req, true) {
                    Ok(sid) => {
                        log(&format!("sent HTTP/3 GET https://{host}/ (stream {sid})"));
                        req_sent = true;
                    }
                    Err(quiche::h3::Error::StreamBlocked) => { /* retry next loop */ }
                    Err(e) => return Err(anyhow::anyhow!("send_request: {e}")),
                }
            }

            // Drain all currently available H3 events.
            loop {
                match h3c.poll(&mut conn) {
                    Ok((sid, Event::Headers { list, .. })) => {
                        for h in &list {
                            match h.name() {
                                b":status" => {
                                    status = std::str::from_utf8(h.value())
                                        .ok()
                                        .and_then(|s| s.parse().ok())
                                        .unwrap_or(0);
                                }
                                b"server" => {
                                    server = String::from_utf8_lossy(h.value()).into_owned();
                                }
                                _ => {}
                            }
                        }
                        log(&format!("H3 headers on stream {sid}: status={status} server={server:?}"));
                    }
                    Ok((sid, Event::Data)) => {
                        while let Ok(read) = h3c.recv_body(&mut conn, sid, &mut buf) {
                            body_len += read;
                            if first_bytes.len() < 120 {
                                let take = (120 - first_bytes.len()).min(read);
                                first_bytes.extend_from_slice(&buf[..take]);
                            }
                        }
                    }
                    Ok((_sid, Event::Finished)) => {
                        log("H3 stream finished");
                        done = true;
                    }
                    Ok((_sid, Event::Reset(code))) => {
                        return Err(anyhow::anyhow!("H3 stream reset: {code}"));
                    }
                    Ok(_) => {}
                    Err(quiche::h3::Error::Done) => break,
                    Err(e) => {
                        log(&format!("h3 poll: {e}"));
                        break;
                    }
                }
            }
        }

        // Flush any egress the ingress/H3 work produced.
        flush(&mut conn, &sock, &mut out).await?;

        if done {
            let _ = conn.close(true, 0x100, b"done");
            let _ = flush(&mut conn, &sock, &mut out).await;
            break;
        }
    }

    let preview = String::from_utf8_lossy(&first_bytes);
    let preview = preview.lines().next().unwrap_or("").trim().to_string();
    let alpn = String::from_utf8_lossy(conn.application_proto()).into_owned();
    let rtt = conn.path_stats().next().map(|p| p.rtt).unwrap_or_default();

    if status == 0 {
        return Err(anyhow::anyhow!(
            "no HTTP/3 response received (established={}, req_sent={req_sent})",
            logged_established
        ));
    }

    let out = format!(
        "HTTP/3 GET OK (quiche): {host} ({peer}) alpn={alpn:?} rtt={rtt:?} status={status} \
         server={server:?} body={body_len}B first_line={preview:?}"
    );
    log(&out);
    log("QUIC-H3-QUICHE-ON-EMSCRIPTEN-OK");
    Ok(out)
}
