//! Shared UDP discovery and keepalive for voice and Go Live.
//!
//! Both connections send the same 74-byte probe. A dropped packet is retried
//! with backoff until the deadline. A garbage datagram is ignored. After the
//! handshake, both connections send Discord's 8-byte UDP ping and notice when
//! the server goes quiet.

use crate::crypto::{discovery_request, valid_discovery_response};
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;
use tokio::net::UdpSocket;

/// Which media connection is probing. The error text uses this name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaRole {
    Voice,
    Stream,
}

impl MediaRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Voice => "voice",
            Self::Stream => "stream",
        }
    }
}

/// `SIO_UDP_CONNRESET`. Windows turns an ICMP port-unreachable into
/// `WSAECONNRESET` unless this is disabled.
pub const SIO_UDP_CONNRESET: u32 = 0x9800_000C;

pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);
pub const PATH_DEAD_AFTER: Duration = Duration::from_secs(15);

pub const DISCOVERY_HINT: &str = "UDP to Discord's media server is blocked or dropping: check the firewall, VPN, or network; mobile hotspot is a good test";

#[derive(Clone, Debug)]
pub struct DiscoverySchedule {
    /// Wait after each probe. The last gap repeats until the deadline.
    pub gaps: Vec<Duration>,
    pub deadline: Duration,
}

impl DiscoverySchedule {
    /// 0 ms, then 250, 500, 1000, and 1500 ms, inside a 9 s budget.
    pub fn production() -> Self {
        Self {
            gaps: vec![
                Duration::from_millis(250),
                Duration::from_millis(500),
                Duration::from_millis(1000),
                Duration::from_millis(1500),
            ],
            deadline: Duration::from_secs(9),
        }
    }

    fn gap(&self, sent: usize) -> Duration {
        self.gaps
            .get(sent.saturating_sub(1))
            .copied()
            .or_else(|| self.gaps.last().copied())
            .unwrap_or(Duration::from_millis(1500))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredAddress {
    pub address: String,
    pub port: u16,
    pub probes: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error(
        "{role} UDP discovery failed for {server} after {attempts} connection attempts and {probes} probes. {DISCOVERY_HINT}"
    )]
    Failed {
        role: MediaRole,
        server: SocketAddr,
        probes: u32,
        attempts: u32,
    },
    #[error("udp: {0}")]
    Io(#[from] std::io::Error),
}

impl DiscoveryError {
    pub fn failed(role: MediaRole, server: SocketAddr, probes: u32, attempts: u32) -> Self {
        Self::Failed {
            role,
            server,
            probes,
            attempts,
        }
    }
}

impl std::fmt::Display for MediaRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Bind an unconnected socket in the server's address family.
pub async fn bind_for(server: SocketAddr) -> std::io::Result<UdpSocket> {
    let local = match server {
        SocketAddr::V4(_) => SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
        SocketAddr::V6(_) => SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)),
    };
    let socket = std::net::UdpSocket::bind(local)?;
    socket.set_nonblocking(true)?;
    #[cfg(windows)]
    disable_udp_connreset(&socket)?;
    UdpSocket::from_std(socket)
}

/// Disable `SIO_UDP_CONNRESET` so ICMP unreachable does not abort `recv`.
#[cfg(windows)]
pub fn disable_udp_connreset(socket: &std::net::UdpSocket) -> std::io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    #[link(name = "ws2_32")]
    extern "system" {
        fn WSAIoctl(
            s: usize,
            control: u32,
            in_buf: *const u8,
            in_len: u32,
            out_buf: *mut u8,
            out_len: u32,
            returned: *mut u32,
            overlapped: *mut u8,
            completion: *mut u8,
        ) -> i32;
    }
    let disable: u32 = 0;
    let mut returned = 0u32;
    let rc = unsafe {
        WSAIoctl(
            socket.as_raw_socket() as usize,
            SIO_UDP_CONNRESET,
            &disable as *const u32 as *const u8,
            4,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn retryable(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        ErrorKind::ConnectionReset | ErrorKind::ConnectionRefused | ErrorKind::Interrupted
    )
}

/// Send discovery probes until a type-`0x0002` reply for `ssrc` arrives from
/// `server`, or the schedule deadline passes.
pub async fn discover(
    socket: &UdpSocket,
    server: SocketAddr,
    ssrc: u32,
    role: MediaRole,
    schedule: &DiscoverySchedule,
) -> Result<DiscoveredAddress, DiscoveryError> {
    let started = tokio::time::Instant::now();
    let mut probes = 0u32;
    let mut buf = [0u8; 256];
    loop {
        if started.elapsed() >= schedule.deadline {
            return Err(DiscoveryError::failed(role, server, probes, 1));
        }
        socket.send_to(&discovery_request(ssrc), server).await?;
        probes += 1;
        let remaining = schedule.deadline.saturating_sub(started.elapsed());
        let slice = schedule.gap(probes as usize).min(remaining);
        let sleep = tokio::time::sleep(slice);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                _ = &mut sleep => break,
                received = socket.recv_from(&mut buf) => {
                    match received {
                        Ok((size, from)) => {
                            if from.ip() != server.ip() || from.port() != server.port() {
                                continue;
                            }
                            if let Some((address, port)) =
                                valid_discovery_response(&buf[..size], ssrc)
                            {
                                return Ok(DiscoveredAddress {
                                    address,
                                    port,
                                    probes,
                                });
                            }
                        }
                        Err(err) if retryable(&err) => continue,
                        Err(err) => return Err(err.into()),
                    }
                }
            }
        }
    }
}

/// Discord UDP keepalive. An 8-byte little-endian counter the server echoes.
pub fn keepalive_packet(sequence: u64) -> [u8; 8] {
    sequence.to_le_bytes()
}

/// Inbound and keepalive clocks shared by voice and stream.
#[derive(Clone, Debug)]
pub struct PathWatch {
    last_inbound: tokio::time::Instant,
    last_keepalive: tokio::time::Instant,
    sequence: u64,
}

impl PathWatch {
    pub fn new() -> Self {
        let now = tokio::time::Instant::now();
        Self {
            last_inbound: now,
            last_keepalive: now,
            sequence: 0,
        }
    }

    pub fn inbound(&mut self) {
        self.last_inbound = tokio::time::Instant::now();
    }

    pub fn keepalive_due(&self) -> bool {
        self.last_keepalive.elapsed() >= KEEPALIVE_INTERVAL
    }

    pub fn dead(&self) -> bool {
        self.last_inbound.elapsed() >= PATH_DEAD_AFTER
    }

    pub fn next_keepalive(&mut self) -> [u8; 8] {
        let packet = keepalive_packet(self.sequence);
        self.sequence = self.sequence.wrapping_add(1);
        self.last_keepalive = tokio::time::Instant::now();
        packet
    }

    /// Test hook. Moves the clocks backward without sleeping.
    pub fn rewind(&mut self, inbound_age: Duration, keepalive_age: Duration) {
        let now = tokio::time::Instant::now();
        self.last_inbound = now
            .checked_sub(inbound_age)
            .unwrap_or_else(tokio::time::Instant::now);
        self.last_keepalive = now
            .checked_sub(keepalive_age)
            .unwrap_or_else(tokio::time::Instant::now);
    }
}

impl Default for PathWatch {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse `ip` and `port` from a voice Ready payload.
pub fn server_addr(ip: &str, port: u16) -> Result<SocketAddr, String> {
    let ip: IpAddr = ip
        .parse()
        .map_err(|err| format!("voice server address {ip}: {err}"))?;
    Ok(SocketAddr::new(ip, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(request: &[u8], port: u16) -> [u8; 74] {
        let mut response = [0u8; 74];
        response[1] = 2;
        response[3] = 70;
        response[4..8].copy_from_slice(&request[4..8]);
        let ip = b"127.0.0.1";
        response[8..8 + ip.len()].copy_from_slice(ip);
        response[72..74].copy_from_slice(&port.to_be_bytes());
        response
    }

    fn fast() -> DiscoverySchedule {
        DiscoverySchedule {
            gaps: vec![Duration::from_millis(15), Duration::from_millis(20)],
            deadline: Duration::from_millis(120),
        }
    }

    async fn serve(drop_first: u32, garbage_first: bool, never: bool) -> SocketAddr {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        tokio::spawn(async move {
            let mut seen = 0u32;
            let mut buf = [0u8; 256];
            loop {
                let Ok((n, peer)) = socket.recv_from(&mut buf).await else {
                    break;
                };
                if never {
                    continue;
                }
                seen += 1;
                if seen <= drop_first {
                    continue;
                }
                if garbage_first && seen == drop_first + 1 {
                    let _ = socket.send_to(&[0xFFu8; 9], peer).await;
                    let _ = socket
                        .send_to(b"not-a-discovery-from-the-right-shape!!", peer)
                        .await;
                }
                if n >= 8 && buf[1] == 1 {
                    let packet = reply(&buf[..n.min(74)], peer.port());
                    let _ = socket.send_to(&packet, peer).await;
                }
            }
        });
        addr
    }

    #[tokio::test]
    async fn discovery_succeeds_after_dropped_probes() {
        let server = serve(2, false, false).await;
        let socket = bind_for(server).await.unwrap();
        assert!(socket.local_addr().unwrap().is_ipv4());
        let found = discover(&socket, server, 77, MediaRole::Voice, &fast())
            .await
            .unwrap();
        assert_eq!(found.address, "127.0.0.1");
        assert!(found.port > 0);
        assert!(found.probes >= 3, "probes {}", found.probes);
    }

    #[tokio::test]
    async fn discovery_ignores_garbage_then_accepts() {
        let server = serve(0, true, false).await;
        let socket = bind_for(server).await.unwrap();
        let found = discover(&socket, server, 41, MediaRole::Stream, &fast())
            .await
            .unwrap();
        assert_eq!(found.address, "127.0.0.1");
        assert_eq!(found.probes, 1);
    }

    #[tokio::test]
    async fn discovery_fails_with_probe_count_when_silent() {
        let server = serve(0, false, true).await;
        let socket = bind_for(server).await.unwrap();
        let err = discover(&socket, server, 9, MediaRole::Voice, &fast())
            .await
            .unwrap_err();
        let text = err.to_string();
        assert!(text.contains("voice UDP discovery failed"), "{text}");
        assert!(text.contains(&server.to_string()), "{text}");
        assert!(text.contains("probes"), "{text}");
        assert!(text.contains(DISCOVERY_HINT), "{text}");
        let DiscoveryError::Failed {
            probes,
            attempts,
            role,
            ..
        } = err
        else {
            panic!("io error");
        };
        assert_eq!(role, MediaRole::Voice);
        assert_eq!(attempts, 1);
        assert!(probes >= 2, "probes {probes}");
    }

    #[test]
    fn production_schedule_covers_eight_to_ten_seconds() {
        let schedule = DiscoverySchedule::production();
        assert_eq!(schedule.gaps[0], Duration::from_millis(250));
        assert_eq!(schedule.gaps[1], Duration::from_millis(500));
        assert_eq!(schedule.gaps[2], Duration::from_millis(1000));
        assert_eq!(schedule.gaps[3], Duration::from_millis(1500));
        let deadline = schedule.deadline;
        assert!(deadline >= Duration::from_secs(8) && deadline <= Duration::from_secs(10));
        let mut elapsed = Duration::ZERO;
        let mut probes = 0u32;
        while elapsed < deadline {
            probes += 1;
            let gap = schedule.gap(probes as usize);
            if elapsed + gap >= deadline {
                break;
            }
            elapsed += gap;
        }
        assert!(probes >= 4, "probes {probes}");
    }

    #[tokio::test]
    async fn bind_matches_the_server_family() {
        let v4 = bind_for(SocketAddr::from((Ipv4Addr::LOCALHOST, 1)))
            .await
            .unwrap();
        assert!(v4.local_addr().unwrap().is_ipv4());
        if let Ok(v6) = bind_for(SocketAddr::from((Ipv6Addr::LOCALHOST, 1))).await {
            assert!(v6.local_addr().unwrap().is_ipv6());
        }
    }

    #[tokio::test]
    async fn stray_sender_is_ignored() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap();
        let stranger = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client = bind_for(server_addr).await.unwrap();
        let client_addr = client.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 128];
            loop {
                let Ok((_, _)) = server.recv_from(&mut buf).await else {
                    break;
                };
                let _ = stranger.send_to(&[9u8; 74], client_addr).await;
                let mut good = [0u8; 74];
                good[1] = 2;
                good[3] = 70;
                good[4..8].copy_from_slice(&buf[4..8]);
                let ip = b"127.0.0.1";
                good[8..8 + ip.len()].copy_from_slice(ip);
                good[72..74].copy_from_slice(&9u16.to_be_bytes());
                let _ = server.send_to(&good, client_addr).await;
            }
        });
        let found = discover(&client, server_addr, 3, MediaRole::Stream, &fast())
            .await
            .unwrap();
        assert_eq!(found.port, 9);
    }

    #[test]
    fn keepalive_is_due_at_five_seconds_and_the_path_dies_at_fifteen() {
        let mut watch = PathWatch::new();
        assert!(!watch.keepalive_due());
        assert!(!watch.dead());
        watch.rewind(Duration::ZERO, KEEPALIVE_INTERVAL);
        assert!(watch.keepalive_due());
        let packet = watch.next_keepalive();
        assert_eq!(packet.len(), 8);
        assert!(!watch.keepalive_due());
        watch.rewind(PATH_DEAD_AFTER, Duration::ZERO);
        assert!(watch.dead());
        watch.inbound();
        assert!(!watch.dead());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_disables_udp_connreset() {
        let std_socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        disable_udp_connreset(&std_socket).unwrap();
        assert_eq!(SIO_UDP_CONNRESET, 0x9800_000C);
    }
}
