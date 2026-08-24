use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use offload_core::{ABI_VERSION, InstancePolicy, OffloadError, OffloadTarget};
use offload_host::{Offloader, WasiConfig};
use offload_remote::{
    Connection, HELLO_ACK_MAGIC, HELLO_MAGIC, PROTOCOL_VERSION, RemoteTarget, Request, Response,
    WireError,
};
use offload_remoted::{SessionOpts, serve};

fn guest_bytes() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_dir.parent().unwrap();
        let guest_dir = workspace_root.join("tests/guests/echo-guest");
        let target_dir = workspace_root.join("target/test-guests-remote");

        let status = Command::new(env!("CARGO"))
            .current_dir(&guest_dir)
            .args(["build", "--locked", "--target", "wasm32-wasip1"])
            .arg("--target-dir")
            .arg(&target_dir)
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .status()
            .expect("failed to spawn cargo for the remote test guest");
        assert!(
            status.success(),
            "guest build failed — is the wasm32-wasip1 target installed? \
             (rustup target add wasm32-wasip1)"
        );

        let wasm = target_dir.join("wasm32-wasip1/debug/offload_test_guest.wasm");
        std::fs::read(&wasm).unwrap_or_else(|error| panic!("read {}: {error}", wasm.display()))
    })
}

fn with_daemon(test: impl FnOnce(SocketAddr), connection_count: usize) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let opts = SessionOpts {
            wasi: WasiConfig::new(),
            pooling: false,
        };
        for _ in 0..connection_count {
            let (stream, _) = listener.accept().unwrap();
            let mut conn = Connection::new(stream);
            let _ = serve(&mut conn, &opts);
        }
    });

    test(addr);
    server.join().unwrap();
}

fn with_offloader(policy: InstancePolicy, test: impl FnOnce(&Offloader)) {
    with_daemon(
        |addr| {
            let offloader = Offloader::builder(guest_bytes())
                .instance_policy(policy)
                .target(RemoteTarget::tcp(addr.to_string()))
                .build()
                .expect("remote offloader build");
            test(&offloader);
        },
        1,
    );
}

fn raw_connection(addr: SocketAddr) -> Connection {
    Connection::new(TcpStream::connect(addr).unwrap())
}

fn handshake(conn: &mut Connection) {
    conn.send(&Request::Hello {
        magic: HELLO_MAGIC,
        protocol_version: PROTOCOL_VERSION,
    })
    .unwrap();
    let response: Response = conn.recv(Duration::from_secs(1)).unwrap();
    assert!(matches!(
        response,
        Response::Hello {
            magic: HELLO_ACK_MAGIC,
            protocol_version: PROTOCOL_VERSION,
        }
    ));
}

type Nested = Vec<Option<(String, u64)>>;

#[test]
fn values_and_multiple_arguments_roundtrip() {
    with_offloader(InstancePolicy::PerCall, |offloader| {
        let value: Nested = vec![
            Some(("hello".into(), 42)),
            None,
            Some((String::new(), u64::MAX)),
            Some(("unicode: αβγ🦀".into(), 0)),
        ];
        let output: Nested = offloader.call("__offload_echo", &(value.clone(),)).unwrap();
        assert_eq!(output, value);

        let output: i64 = offloader.call("__offload_add", &(40i64, 2i64)).unwrap();
        assert_eq!(output, 42);
    });
}

#[test]
fn guest_trap_and_missing_export_preserve_error_variants() {
    with_offloader(InstancePolicy::PerCall, |offloader| {
        let error = offloader
            .call::<_, ()>("__offload_panics", &("boom".to_string(),))
            .unwrap_err();
        assert!(
            matches!(error, OffloadError::GuestTrap(_)),
            "expected GuestTrap, got: {error}"
        );

        let error = offloader
            .call::<_, ()>("__offload_no_such_fn", &())
            .unwrap_err();
        assert!(matches!(
            error,
            OffloadError::MissingExport(ref export) if export == "__offload_no_such_fn"
        ));
    });
}

#[test]
fn per_call_resets_guest_state() {
    with_offloader(InstancePolicy::PerCall, |offloader| {
        for _ in 0..3 {
            let value: u64 = offloader.call("__offload_bump", &()).unwrap();
            assert_eq!(value, 1);
        }
    });
}

#[test]
fn shared_persists_guest_state() {
    with_offloader(InstancePolicy::Shared, |offloader| {
        for expected in 1..=3u64 {
            let value: u64 = offloader.call("__offload_bump", &()).unwrap();
            assert_eq!(value, expected);
        }
    });
}

#[test]
fn shared_discards_guest_state_after_trap() {
    with_offloader(InstancePolicy::Shared, |offloader| {
        let value: u64 = offloader.call("__offload_bump", &()).unwrap();
        assert_eq!(value, 1);
        let value: u64 = offloader.call("__offload_bump", &()).unwrap();
        assert_eq!(value, 2);

        let error = offloader
            .call::<_, ()>("__offload_panics", &("boom".to_string(),))
            .unwrap_err();
        assert!(matches!(error, OffloadError::GuestTrap(_)));

        let value: u64 = offloader.call("__offload_bump", &()).unwrap();
        assert_eq!(value, 1);
    });
}

#[test]
fn concurrent_calls_are_serialized_safely() {
    with_daemon(
        |addr| {
            let offloader = Arc::new(
                Offloader::builder(guest_bytes())
                    .instance_policy(InstancePolicy::PerCall)
                    .target(RemoteTarget::tcp(addr.to_string()))
                    .build()
                    .unwrap(),
            );
            let handles: Vec<_> = (0..8)
                .map(|thread_index| {
                    let offloader = Arc::clone(&offloader);
                    std::thread::spawn(move || {
                        for value in 0..20i64 {
                            let output: i64 = offloader
                                .call("__offload_add", &(thread_index as i64, value))
                                .unwrap();
                            assert_eq!(output, thread_index as i64 + value);
                        }
                    })
                })
                .collect();
            for handle in handles {
                handle.join().unwrap();
            }
        },
        1,
    );
}

#[test]
fn version_mismatch_is_rejected_by_daemon_and_connection_closes() {
    with_daemon(
        |addr| {
            let mut conn = raw_connection(addr);
            conn.send(&Request::Hello {
                magic: HELLO_MAGIC,
                protocol_version: PROTOCOL_VERSION + 1,
            })
            .unwrap();
            let response: Response = conn.recv(Duration::from_secs(1)).unwrap();
            assert!(matches!(
                response,
                Response::Err(WireError::Protocol(ref message))
                    if message.contains("handshake mismatch")
            ));
            let error = conn.recv::<Response>(Duration::from_secs(1)).unwrap_err();
            assert!(matches!(error, offload_remote::ConnError::Disconnected));
        },
        1,
    );
}

#[test]
fn invalid_daemon_ack_is_a_transport_error() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut conn = Connection::new(stream);
        let request: Request = conn.recv_blocking().unwrap();
        assert!(matches!(request, Request::Hello { .. }));
        conn.send(&Response::Hello {
            magic: HELLO_ACK_MAGIC,
            protocol_version: PROTOCOL_VERSION + 1,
        })
        .unwrap();
    });

    let mut target = RemoteTarget::tcp(addr.to_string());
    let error = target
        .prepare(b"unused", InstancePolicy::PerCall)
        .unwrap_err();
    assert!(matches!(error, OffloadError::Transport(_)));
    server.join().unwrap();
}

#[test]
fn daemon_death_mid_session_is_a_transport_error() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut conn = Connection::new(stream);
        handshake_as_server(&mut conn);
        let request: Request = conn.recv_blocking().unwrap();
        assert!(matches!(request, Request::Prepare { .. }));
        conn.send(&Response::Prepared).unwrap();
        let request: Request = conn.recv_blocking().unwrap();
        assert!(matches!(request, Request::AbiVersion));
        conn.send(&Response::AbiVersion(ABI_VERSION)).unwrap();
    });

    let offloader = Offloader::builder(guest_bytes())
        .target(RemoteTarget::tcp(addr.to_string()))
        .build()
        .unwrap();
    server.join().unwrap();
    let error = offloader
        .call::<_, i64>("__offload_add", &(20i64, 22i64))
        .unwrap_err();
    assert!(matches!(error, OffloadError::Transport(_)));
}

#[test]
fn call_before_prepare_is_a_protocol_error() {
    with_daemon(
        |addr| {
            let mut conn = raw_connection(addr);
            handshake(&mut conn);
            conn.send(&Request::Call {
                export: "__offload_add".into(),
                args: Vec::new(),
            })
            .unwrap();
            let response: Response = conn.recv(Duration::from_secs(1)).unwrap();
            assert!(matches!(
                response,
                Response::Err(WireError::Protocol(ref message))
                    if message == "Call received before Prepare"
            ));
        },
        1,
    );
}

#[test]
fn hello_restarts_a_persistent_transport_session() {
    with_daemon(
        |addr| {
            let mut conn = raw_connection(addr);
            handshake(&mut conn);
            handshake(&mut conn);
        },
        1,
    );
}

#[test]
fn reconnect_starts_with_a_fresh_session() {
    with_daemon(
        |addr| {
            let mut first = raw_connection(addr);
            handshake(&mut first);
            first
                .send(&Request::Prepare {
                    module: guest_bytes().to_vec(),
                    policy: InstancePolicy::PerCall,
                })
                .unwrap();
            let response: Response = first.recv(Duration::from_secs(300)).unwrap();
            assert!(matches!(response, Response::Prepared));
            drop(first);

            let mut second = raw_connection(addr);
            handshake(&mut second);
            second
                .send(&Request::Call {
                    export: "__offload_add".into(),
                    args: Vec::new(),
                })
                .unwrap();
            let response: Response = second.recv(Duration::from_secs(1)).unwrap();
            assert!(matches!(
                response,
                Response::Err(WireError::Protocol(ref message))
                    if message == "Call received before Prepare"
            ));
        },
        2,
    );
}

fn handshake_as_server(conn: &mut Connection) {
    let request: Request = conn.recv_blocking().unwrap();
    assert!(matches!(
        request,
        Request::Hello {
            magic: HELLO_MAGIC,
            protocol_version: PROTOCOL_VERSION,
        }
    ));
    conn.send(&Response::Hello {
        magic: HELLO_ACK_MAGIC,
        protocol_version: PROTOCOL_VERSION,
    })
    .unwrap();
}
