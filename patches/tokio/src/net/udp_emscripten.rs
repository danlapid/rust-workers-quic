//! `tokio::net::UdpSocket` for `wasm32-unknown-emscripten`, the reactor-backed
//! counterpart of the mio/socket2 implementation.
//!
//! Like the emscripten [`TcpStream`](crate::net::TcpStream) and
//! [`UnixDatagram`](crate::net::unix::UnixDatagram), this is a thin shell over
//! the shared [`ReactorStream`] (a `PollEvented` over the emscripten reactor
//! [`Source`]). Addressing, `bind`, and `connect` reuse `std::net::UdpSocket`
//! (which compiles on emscripten); only the async readiness layer is bespoke.
//! The datagrams themselves are carried by emscripten's `-sNODERAWSOCKETS`
//! layer over `node:dgram`.
//!
//! This is the UDP primitive QUIC rides on: a `quinn::AsyncUdpSocket` adapter is
//! built directly on `poll_send_to` / `poll_recv_from`.
//!
//! [`ReactorStream`]: crate::net::reactor_stream::ReactorStream
//! [`Source`]: crate::runtime::io::Source

use crate::io::{Interest, ReadBuf, Ready};
use crate::net::reactor_stream::ReactorStream;
use crate::net::ToSocketAddrs;

use std::fmt;
use std::io;
use std::net::{self as std_net, SocketAddr};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, RawFd};
use std::task::{Context, Poll};

/// Borrow `fd` as a `std::net::UdpSocket` for the duration of `f` without taking
/// ownership — the fd is released (not closed) before returning.
fn with_std<R>(fd: RawFd, f: impl FnOnce(&std_net::UdpSocket) -> R) -> R {
    // SAFETY: `fd` is owned by the reactor `Source` for the call; we hand it
    // back via `into_raw_fd` so it is never double-closed.
    let sock = unsafe { std_net::UdpSocket::from_raw_fd(fd) };
    let ret = f(&sock);
    let _ = sock.into_raw_fd();
    ret
}

/// A UDP socket, the emscripten counterpart of the mio-backed
/// [`crate::net::UdpSocket`].
pub struct UdpSocket {
    inner: ReactorStream,
}

impl UdpSocket {
    /// Binds to one of `addr`'s socket addresses.
    pub async fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<UdpSocket> {
        let addrs = crate::net::to_socket_addrs(addr).await?;
        let mut last_err = None;
        for a in addrs {
            match std_net::UdpSocket::bind(a) {
                Ok(sock) => return UdpSocket::from_std(sock),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "no addresses to bind to")
        }))
    }

    /// Registers a `std` UDP socket with the reactor (setting it non-blocking).
    pub fn from_std(socket: std_net::UdpSocket) -> io::Result<UdpSocket> {
        socket.set_nonblocking(true)?;
        Ok(UdpSocket {
            inner: ReactorStream::from_raw_fd(socket.into_raw_fd())?,
        })
    }

    /// Deregisters and returns the inner `std` socket.
    pub fn into_std(self) -> io::Result<std_net::UdpSocket> {
        // SAFETY: we own the fd, just released from the reactor.
        Ok(unsafe { std_net::UdpSocket::from_raw_fd(self.inner.into_raw_fd()?) })
    }

    /// Returns the local address this socket is bound to.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        with_std(self.inner.as_raw_fd(), |s| s.local_addr())
    }

    /// Returns the connected peer address, if any.
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        with_std(self.inner.as_raw_fd(), |s| s.peer_addr())
    }

    /// Connects the socket to a remote address so `send`/`recv` can be used.
    pub async fn connect<A: ToSocketAddrs>(&self, addr: A) -> io::Result<()> {
        let addrs = crate::net::to_socket_addrs(addr).await?;
        let fd = self.inner.as_raw_fd();
        let mut last_err = None;
        for a in addrs {
            match with_std(fd, |s| s.connect(a)) {
                Ok(()) => return Ok(()),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "no addresses to connect to")
        }))
    }

    /// Returns any pending `SO_ERROR`.
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        with_std(self.inner.as_raw_fd(), |s| s.take_error())
    }

    // ===== readiness =====

    /// Waits for any of `interest` to become ready.
    pub async fn ready(&self, interest: Interest) -> io::Result<Ready> {
        self.inner.ready(interest).await
    }

    /// Waits for the socket to become writable (send-ready).
    pub async fn writable(&self) -> io::Result<()> {
        self.ready(Interest::WRITABLE).await.map(|_| ())
    }

    /// Waits for the socket to become readable (recv-ready).
    pub async fn readable(&self) -> io::Result<()> {
        self.ready(Interest::READABLE).await.map(|_| ())
    }

    /// Polls for send readiness.
    pub fn poll_send_ready(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner.poll_write_ready(cx)
    }

    /// Polls for recv readiness.
    pub fn poll_recv_ready(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner.poll_read_ready(cx)
    }

    // ===== connected send/recv =====

    /// Sends on a connected socket.
    pub async fn send(&self, buf: &[u8]) -> io::Result<usize> {
        let fd = self.inner.as_raw_fd();
        self.inner
            .async_io(Interest::WRITABLE, || with_std(fd, |s| s.send(buf)))
            .await
    }

    /// Receives on a connected socket.
    pub async fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        let fd = self.inner.as_raw_fd();
        self.inner
            .async_io(Interest::READABLE, || with_std(fd, |s| s.recv(buf)))
            .await
    }

    // ===== unconnected send_to/recv_from =====

    /// Sends `buf` to `target`.
    pub async fn send_to<A: ToSocketAddrs>(&self, buf: &[u8], target: A) -> io::Result<usize> {
        let mut addrs = crate::net::to_socket_addrs(target).await?;
        let target = addrs
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no addresses"))?;
        let fd = self.inner.as_raw_fd();
        self.inner
            .async_io(Interest::WRITABLE, || with_std(fd, |s| s.send_to(buf, target)))
            .await
    }

    /// Receives a single datagram, returning the sender address.
    pub async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let fd = self.inner.as_raw_fd();
        self.inner
            .async_io(Interest::READABLE, || with_std(fd, |s| s.recv_from(buf)))
            .await
    }

    // ===== try_* (readiness-gated, non-blocking) =====

    /// Tries to send to `target` without waiting.
    pub fn try_send_to(&self, buf: &[u8], target: SocketAddr) -> io::Result<usize> {
        let fd = self.inner.as_raw_fd();
        self.inner
            .try_io(Interest::WRITABLE, || with_std(fd, |s| s.send_to(buf, target)))
    }

    /// Tries to receive a datagram without waiting.
    pub fn try_recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let fd = self.inner.as_raw_fd();
        self.inner
            .try_io(Interest::READABLE, || with_std(fd, |s| s.recv_from(buf)))
    }

    /// Tries to send on a connected socket without waiting.
    pub fn try_send(&self, buf: &[u8]) -> io::Result<usize> {
        let fd = self.inner.as_raw_fd();
        self.inner
            .try_io(Interest::WRITABLE, || with_std(fd, |s| s.send(buf)))
    }

    /// Tries to receive on a connected socket without waiting.
    pub fn try_recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        let fd = self.inner.as_raw_fd();
        self.inner
            .try_io(Interest::READABLE, || with_std(fd, |s| s.recv(buf)))
    }

    /// Runs `f` against the current readiness for `interest`, returning
    /// `WouldBlock` if not ready. Mirrors the mio-backed `UdpSocket::try_io`
    /// (used by quinn's tokio runtime adapter).
    pub fn try_io<R>(
        &self,
        interest: Interest,
        f: impl FnOnce() -> io::Result<R>,
    ) -> io::Result<R> {
        self.inner.try_io(interest, f)
    }

    /// Awaits readiness for `interest`, then runs `f`.
    pub async fn async_io<R>(
        &self,
        interest: Interest,
        f: impl FnMut() -> io::Result<R>,
    ) -> io::Result<R> {
        self.inner.async_io(interest, f).await
    }

    // ===== poll_* (the surface the quinn AsyncUdpSocket adapter drives) =====

    /// Polls sending `buf` to `target`.
    pub fn poll_send_to(
        &self,
        cx: &mut Context<'_>,
        buf: &[u8],
        target: SocketAddr,
    ) -> Poll<io::Result<usize>> {
        let fd = self.inner.as_raw_fd();
        self.inner
            .poll_write_io(cx, || with_std(fd, |s| s.send_to(buf, target)))
    }

    /// Polls sending `buf` on a connected socket.
    pub fn poll_send(&self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let fd = self.inner.as_raw_fd();
        self.inner.poll_write_io(cx, || with_std(fd, |s| s.send(buf)))
    }

    /// Polls receiving a datagram into `buf`, returning the sender address.
    pub fn poll_recv_from(
        &self,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<SocketAddr>> {
        let fd = self.inner.as_raw_fd();
        self.inner.poll_read_io(cx, || {
            // `initialize_unfilled` zeroes the unfilled region and hands back an
            // `&mut [u8]`; the borrow ends before we `advance`.
            let n;
            let addr;
            {
                let dst = buf.initialize_unfilled();
                let (got, from) = with_std(fd, |s| s.recv_from(dst))?;
                n = got;
                addr = from;
            }
            buf.advance(n);
            Ok(addr)
        })
    }

    /// Polls receiving on a connected socket into `buf`.
    pub fn poll_recv(&self, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let fd = self.inner.as_raw_fd();
        self.inner.poll_read_io(cx, || {
            let n;
            {
                let dst = buf.initialize_unfilled();
                n = with_std(fd, |s| s.recv(dst))?;
            }
            buf.advance(n);
            Ok(())
        })
    }
}

impl fmt::Debug for UdpSocket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UdpSocket")
            .field("fd", &self.inner.as_raw_fd())
            .finish()
    }
}

impl AsRawFd for UdpSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}

impl AsFd for UdpSocket {
    fn as_fd(&self) -> BorrowedFd<'_> {
        // SAFETY: the fd is owned by `self.inner` for `self`'s lifetime.
        unsafe { BorrowedFd::borrow_raw(self.as_raw_fd()) }
    }
}
