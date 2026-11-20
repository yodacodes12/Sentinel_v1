//! The IPC server: accepts connections and speaks the line-delimited protocol.
//!
//! Hostile or broken input is expected. Every connection is bounded in frame
//! size, idle time and count, and only the daemon's own user may connect.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use sentinel_core::platform::current_uid;
use sentinel_ipc::codec::{decode_request, encode};
use sentinel_ipc::{ErrorCode, IpcError, MAX_FRAME_BYTES, ResponseEnvelope};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::OwnedReadHalf;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::context::DaemonContext;
use crate::handler::{Effect, handle};

pub type SharedContext = Arc<Mutex<DaemonContext>>;

#[derive(Debug, Clone, Copy)]
pub struct ServerLimits {
    /// Connections served at once; further ones are closed immediately.
    pub max_connections: usize,
    /// A connection that sends nothing (or an unfinished frame) for this
    /// long is closed.
    pub idle_timeout: Duration,
    /// How long shutdown waits for in-flight connections to finish.
    pub drain_timeout: Duration,
}

impl Default for ServerLimits {
    fn default() -> Self {
        Self {
            max_connections: 32,
            idle_timeout: Duration::from_secs(30),
            drain_timeout: Duration::from_secs(2),
        }
    }
}

/// Serve until `shutdown` becomes true, then let open connections finish.
pub async fn serve(
    listener: &UnixListener,
    ctx: SharedContext,
    shutdown: watch::Sender<bool>,
    limits: ServerLimits,
) {
    let mut stop = shutdown.subscribe();
    let mut connections = JoinSet::new();
    let own_uid = current_uid();

    loop {
        tokio::select! {
            _ = stop.wait_for(|stopping| *stopping) => break,
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    if !peer_is_owner(&stream, own_uid) {
                        tracing::warn!("rejected an IPC connection from another user");
                    } else if connections.len() >= limits.max_connections {
                        tracing::warn!(
                            limit = limits.max_connections,
                            "connection limit reached; rejecting a connection"
                        );
                    } else {
                        connections.spawn(connection(stream, ctx.clone(), shutdown.clone(), limits));
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "accepting an IPC connection failed");
                    // Persistent failures (e.g. descriptor exhaustion) must
                    // not turn the accept loop into a busy loop.
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }
    }

    let drained = timeout(limits.drain_timeout, async {
        while connections.join_next().await.is_some() {}
    })
    .await;
    if drained.is_err() {
        tracing::warn!("closing connections that did not finish in time");
        connections.shutdown().await;
    }
}

/// Only the user who runs the daemon may talk to it, checked with
/// `SO_PEERCRED` in addition to the filesystem permissions.
fn peer_is_owner(stream: &UnixStream, own_uid: u32) -> bool {
    matches!(stream.peer_cred(), Ok(cred) if cred.uid() == own_uid)
}

enum Frame {
    Line(Vec<u8>),
    TooLarge,
    Closed,
}

async fn read_frame(reader: &mut BufReader<OwnedReadHalf>) -> std::io::Result<Frame> {
    let mut buffer = Vec::new();
    // Never buffer more than one maximum frame plus its newline.
    let read = reader
        .take(MAX_FRAME_BYTES as u64 + 1)
        .read_until(b'\n', &mut buffer)
        .await?;
    if read == 0 {
        return Ok(Frame::Closed);
    }
    if buffer.last() == Some(&b'\n') {
        buffer.pop();
        Ok(Frame::Line(buffer))
    } else if buffer.len() > MAX_FRAME_BYTES {
        Ok(Frame::TooLarge)
    } else {
        // The peer closed mid-frame: there is nobody left to answer.
        Ok(Frame::Closed)
    }
}

async fn connection(
    stream: UnixStream,
    ctx: SharedContext,
    shutdown: watch::Sender<bool>,
    limits: ServerLimits,
) {
    let mut stop = shutdown.subscribe();
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    loop {
        let frame = tokio::select! {
            _ = stop.wait_for(|stopping| *stopping) => break,
            read = timeout(limits.idle_timeout, read_frame(&mut reader)) => match read {
                Ok(Ok(frame)) => frame,
                Ok(Err(error)) => {
                    tracing::debug!(%error, "IPC read failed");
                    break;
                }
                Err(_) => break, // idle
            }
        };

        let (envelope, effect, close) = match frame {
            Frame::Closed => break,
            Frame::TooLarge => (
                ResponseEnvelope::new(
                    None,
                    Err(IpcError::new(
                        ErrorCode::FrameTooLarge,
                        format!("message exceeds {MAX_FRAME_BYTES} bytes"),
                    )),
                ),
                Effect::None,
                // The rest of the oversized frame is still in the stream, so
                // the connection cannot be resynchronised.
                true,
            ),
            Frame::Line(bytes) => match decode_request(&bytes) {
                Err(failure) => {
                    tracing::debug!(
                        code = failure.error.code.as_str(),
                        "rejected an IPC message"
                    );
                    (
                        ResponseEnvelope::new(failure.id, Err(failure.error)),
                        Effect::None,
                        false,
                    )
                }
                Ok(request) => {
                    // The lock is never held across an await point.
                    let (result, effect) = {
                        let mut ctx = ctx.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        handle(&mut ctx, request.request)
                    };
                    (
                        ResponseEnvelope::new(Some(request.id), result),
                        effect,
                        false,
                    )
                }
            },
        };

        if write_half.write_all(&encode(&envelope)).await.is_err() {
            break;
        }
        if effect == Effect::Shutdown {
            let _ = shutdown.send(true);
            break;
        }
        if close {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sentinel_config::ConfigStore;
    use sentinel_core::{DaemonState, SentinelError};
    use sentinel_ipc::{Client, Outcome, Request, RequestEnvelope};
    use std::io::{BufRead, Read, Write};
    use std::path::PathBuf;
    use std::thread::JoinHandle;

    /// A real server on its own thread and runtime, so tests can drive it
    /// with the same blocking client the CLI and desktop use.
    struct TestServer {
        _dir: tempfile::TempDir,
        socket: PathBuf,
        shutdown: watch::Sender<bool>,
        thread: Option<JoinHandle<()>>,
    }

    impl TestServer {
        fn start(limits: ServerLimits) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let socket = dir.path().join("daemon.sock");
            let config = ConfigStore::open(&dir.path().join("config.toml")).unwrap();
            let mut ctx = DaemonContext::new(
                config,
                crate::handler::tests::test_monitor(dir.path()),
                crate::handler::tests::test_discovery(dir.path()),
            );
            ctx.transition(DaemonState::Running).unwrap();
            let ctx = Arc::new(Mutex::new(ctx));
            let (shutdown, _) = watch::channel(false);

            let (ready_tx, ready_rx) = std::sync::mpsc::channel();
            let thread = {
                let socket = socket.clone();
                let shutdown = shutdown.clone();
                std::thread::spawn(move || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();
                    runtime.block_on(async move {
                        let bound = crate::socket::BoundSocket::bind(&socket).unwrap();
                        ready_tx.send(()).unwrap();
                        serve(bound.listener(), ctx, shutdown, limits).await;
                    });
                })
            };
            ready_rx.recv().unwrap();
            Self {
                _dir: dir,
                socket,
                shutdown,
                thread: Some(thread),
            }
        }

        fn client(&self) -> Client {
            Client::connect(&self.socket).unwrap()
        }

        fn raw(&self) -> std::io::BufReader<std::os::unix::net::UnixStream> {
            let stream = std::os::unix::net::UnixStream::connect(&self.socket).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            std::io::BufReader::new(stream)
        }

        fn join(&mut self) {
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            let _ = self.shutdown.send(true);
            self.join();
        }
    }

    fn exchange(
        raw: &mut std::io::BufReader<std::os::unix::net::UnixStream>,
        line: &[u8],
    ) -> ResponseEnvelope {
        raw.get_mut().write_all(line).unwrap();
        raw.get_mut().write_all(b"\n").unwrap();
        let mut answer = String::new();
        raw.read_line(&mut answer).unwrap();
        serde_json::from_str(&answer).unwrap_or_else(|e| panic!("bad answer {answer:?}: {e}"))
    }

    fn error_code(envelope: &ResponseEnvelope) -> ErrorCode {
        match &envelope.outcome {
            Outcome::Error { error } => error.code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn answers_status_requests() {
        let server = TestServer::start(ServerLimits::default());
        let status = server.client().status().unwrap();
        assert_eq!(status.state, DaemonState::Running);
        assert_eq!(status.pid, std::process::id());
    }

    #[test]
    fn serves_multiple_clients_at_once() {
        let server = TestServer::start(ServerLimits::default());
        let mut clients: Vec<Client> = (0..8).map(|_| server.client()).collect();
        // Interleave requests across all open connections.
        for round in 0..3 {
            for client in clients.iter_mut() {
                let status = client.status().unwrap();
                assert_eq!(status.state, DaemonState::Running, "round {round}");
            }
        }
        // And from separate threads simultaneously.
        let socket = server.socket.clone();
        let workers: Vec<_> = (0..6)
            .map(|_| {
                let socket = socket.clone();
                std::thread::spawn(move || {
                    let mut client = Client::connect(&socket).unwrap();
                    for _ in 0..20 {
                        client.application_version().unwrap();
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
    }

    #[test]
    fn malformed_messages_get_errors_and_the_connection_survives() {
        let server = TestServer::start(ServerLimits::default());
        let mut raw = server.raw();

        let answer = exchange(&mut raw, b"this is not json");
        assert_eq!(error_code(&answer), ErrorCode::MalformedRequest);
        assert_eq!(answer.id, None);

        let answer = exchange(
            &mut raw,
            b"{\"version\":7,\"id\":5,\"request\":{\"type\":\"Shutdown\"}}",
        );
        assert_eq!(error_code(&answer), ErrorCode::UnsupportedVersion);
        assert_eq!(answer.id, Some(5));

        let answer = exchange(
            &mut raw,
            b"{\"version\":1,\"id\":6,\"request\":{\"type\":\"RunCommand\",\"argv\":[\"id\"]}}",
        );
        assert_eq!(error_code(&answer), ErrorCode::InvalidRequest);

        // The same connection still works for a valid request.
        let frame = encode(&RequestEnvelope::new(7, Request::GetDaemonStatus));
        let answer = exchange(&mut raw, &frame[..frame.len() - 1]);
        assert!(matches!(answer.outcome, Outcome::Ok { .. }));
        assert_eq!(answer.id, Some(7));
    }

    #[test]
    fn oversized_frame_is_rejected_and_the_connection_closed() {
        let server = TestServer::start(ServerLimits::default());
        let mut raw = server.raw();
        let junk = vec![b'x'; MAX_FRAME_BYTES + 4096];
        // The server may close before we finish writing; that is fine.
        let _ = raw.get_mut().write_all(&junk);
        let mut answer = String::new();
        let _ = raw.read_line(&mut answer);
        if !answer.is_empty() {
            let envelope: ResponseEnvelope = serde_json::from_str(&answer).unwrap();
            assert_eq!(error_code(&envelope), ErrorCode::FrameTooLarge);
        }
        // Either way the server is still healthy for other clients.
        assert!(server.client().status().is_ok());
    }

    #[test]
    fn abrupt_disconnects_do_not_disturb_the_server() {
        let server = TestServer::start(ServerLimits::default());
        for _ in 0..20 {
            let mut raw = server.raw();
            raw.get_mut().write_all(b"{\"version\":1,\"id\"").unwrap();
            drop(raw); // gone mid-frame
        }
        drop(server.raw()); // connected, said nothing
        assert!(server.client().status().is_ok());
    }

    #[test]
    fn idle_connections_are_closed() {
        let server = TestServer::start(ServerLimits {
            idle_timeout: Duration::from_millis(100),
            ..Default::default()
        });
        let mut raw = server.raw();
        let mut buffer = [0u8; 1];
        // A clean EOF (0 bytes) means the server hung up on us.
        assert_eq!(raw.get_mut().read(&mut buffer).unwrap(), 0);
    }

    #[test]
    fn connection_limit_is_enforced() {
        let server = TestServer::start(ServerLimits {
            max_connections: 2,
            ..Default::default()
        });
        let mut first = server.client();
        let mut second = server.client();
        first.status().unwrap();
        second.status().unwrap();

        let mut third = server.client(); // connects, then is dropped by the server
        assert!(matches!(third.status(), Err(SentinelError::Ipc(_))));

        drop(first);
        // A slot frees up once the server notices the disconnect.
        let recovered = (0..50).any(|_| {
            std::thread::sleep(Duration::from_millis(20));
            Client::connect(&server.socket)
                .and_then(|mut c| c.status())
                .is_ok()
        });
        assert!(recovered);
    }

    #[test]
    fn shutdown_request_is_answered_then_the_server_stops() {
        let mut server = TestServer::start(ServerLimits::default());
        let mut bystander = server.client();
        bystander.status().unwrap();

        server
            .client()
            .shutdown()
            .expect("shutdown is acknowledged");
        server.join(); // serve() returned and the socket was removed

        assert!(!server.socket.exists(), "socket file must be cleaned up");
        assert!(matches!(
            Client::connect(&server.socket),
            Err(SentinelError::DaemonUnavailable)
        ));
        // The other client sees a clean disconnect, not a hang.
        assert!(bystander.status().is_err());
    }
}
