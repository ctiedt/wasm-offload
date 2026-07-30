use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use offload_core::ABI_VERSION;
use offload_host::{InstancePolicy, OffloadError, Offloader};
use proptest::prelude::*;

fn guest_bytes() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_dir.parent().unwrap();
        let guest_dir = workspace_root.join("tests/guests/echo-guest");
        let target_dir = workspace_root.join("target/test-guests");

        let status = Command::new(env!("CARGO"))
            .current_dir(&guest_dir)
            .args(["build", "--locked", "--target", "wasm32-wasip1"])
            .arg("--target-dir")
            .arg(&target_dir)
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .status()
            .expect("failed to spawn cargo for the test guest");
        assert!(
            status.success(),
            "guest build failed — is the wasm32-wasip1 target installed? \
             (rustup target add wasm32-wasip1)"
        );

        let wasm = target_dir.join("wasm32-wasip1/debug/offload_test_guest.wasm");
        std::fs::read(&wasm).unwrap_or_else(|e| panic!("read {}: {e}", wasm.display()))
    })
}

fn offloader(policy: InstancePolicy) -> Offloader {
    Offloader::builder(guest_bytes())
        .instance_policy(policy)
        .build()
        .expect("offloader build")
}

type Nested = Vec<Option<(String, u64)>>;

#[test]
fn nested_struct_roundtrip() {
    let off = offloader(InstancePolicy::PerCall);
    let v: Nested = vec![
        Some(("hello".into(), 42)),
        None,
        Some((String::new(), u64::MAX)),
        Some(("unicode: αβγ🦀".into(), 0)),
    ];
    let out: Nested = off.call("__offload_echo", &(v.clone(),)).unwrap();
    assert_eq!(out, v);
}

#[test]
fn multi_arg_call() {
    let off = offloader(InstancePolicy::PerCall);
    let out: i64 = off.call("__offload_add", &(40i64, 2i64)).unwrap();
    assert_eq!(out, 42);
    let out: i64 = off.call("__offload_add", &(i64::MAX, 1i64)).unwrap();
    assert_eq!(out, i64::MIN);
}

#[test]
fn zero_arg_unit_return() {
    let off = offloader(InstancePolicy::PerCall);
    let () = off.call("__offload_nothing", &()).unwrap();
}

#[test]
fn guest_panic_is_guest_trap() {
    let off = offloader(InstancePolicy::PerCall);
    let err = off
        .call::<_, ()>("__offload_panics", &("boom".to_string(),))
        .unwrap_err();
    assert!(
        matches!(err, OffloadError::GuestTrap(_)),
        "expected GuestTrap, got: {err}"
    );
}

#[test]
fn missing_export_is_reported() {
    let off = offloader(InstancePolicy::PerCall);
    let err = off.call::<_, ()>("__offload_no_such_fn", &()).unwrap_err();
    match err {
        OffloadError::MissingExport(name) => assert_eq!(name, "__offload_no_such_fn"),
        other => panic!("expected MissingExport, got: {other}"),
    }
}

#[test]
fn out_of_bounds_return_is_rejected_before_host_allocation() {
    let off = offloader(InstancePolicy::PerCall);
    let error = off
        .call::<_, ()>("__offload_invalid_return", &())
        .unwrap_err();
    assert!(matches!(error, OffloadError::Runtime(_)));
}

#[test]
fn per_call_resets_guest_state() {
    let off = offloader(InstancePolicy::PerCall);
    for _ in 0..3 {
        let n: u64 = off.call("__offload_bump", &()).unwrap();
        assert_eq!(n, 1);
    }
}

#[test]
fn shared_persists_guest_state() {
    let off = offloader(InstancePolicy::Shared);
    for expected in 1..=3u64 {
        let n: u64 = off.call("__offload_bump", &()).unwrap();
        assert_eq!(n, expected);
    }
}

#[test]
fn shared_discards_instance_after_trap() {
    let off = offloader(InstancePolicy::Shared);
    let n: u64 = off.call("__offload_bump", &()).unwrap();
    assert_eq!(n, 1);
    let n: u64 = off.call("__offload_bump", &()).unwrap();
    assert_eq!(n, 2);

    let err = off
        .call::<_, ()>("__offload_panics", &("boom".to_string(),))
        .unwrap_err();
    assert!(matches!(err, OffloadError::GuestTrap(_)));

    let n: u64 = off.call("__offload_bump", &()).unwrap();
    assert_eq!(n, 1, "post-trap call must run on a fresh instance");
    let n: u64 = off.call("__offload_bump", &()).unwrap();
    assert_eq!(n, 2);
}

#[test]
fn wrong_arity_call_is_rejected() {
    let off = offloader(InstancePolicy::PerCall);
    let err = off
        .call::<_, i64>("__offload_add", &(1i64, 2i64, 3i64))
        .unwrap_err();
    assert!(
        matches!(err, OffloadError::GuestTrap(_)),
        "expected GuestTrap from strict argument decode, got: {err}"
    );
}

#[test]
fn per_call_is_parallel() {
    let off = std::sync::Arc::new(offloader(InstancePolicy::PerCall));
    let handles: Vec<_> = (0..8)
        .map(|t| {
            let off = off.clone();
            std::thread::spawn(move || {
                for i in 0..20i64 {
                    let out: i64 = off.call("__offload_add", &(t as i64, i)).unwrap();
                    assert_eq!(out, t as i64 + i);
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn abi_version_handshake_present() {
    assert_eq!(ABI_VERSION, 1);
    offloader(InstancePolicy::PerCall);
}

#[test]
fn pooling_allocator_roundtrip() {
    let off = Offloader::builder(guest_bytes())
        .pooling_allocator(true)
        .build()
        .expect("pooling offloader build");
    let out: i64 = off.call("__offload_add", &(20i64, 22i64)).unwrap();
    assert_eq!(out, 42);
}

#[test]
fn guest_exports_are_present_in_artifact() {
    let mut exports = std::collections::HashSet::new();
    for payload in wasmparser::Parser::new(0).parse_all(guest_bytes()) {
        if let wasmparser::Payload::ExportSection(reader) = payload.unwrap() {
            for export in reader {
                exports.insert(export.unwrap().name.to_string());
            }
        }
    }
    for required in [
        "memory",
        "__offload_alloc",
        "__offload_free",
        "__offload_abi_version",
        "__offload_echo",
        "__offload_add",
        "__offload_echo_bytes",
        "__offload_nothing",
        "__offload_panics",
        "__offload_bump",
        "__offload_invalid_return",
    ] {
        assert!(exports.contains(required), "missing export: {required}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn echo_roundtrips_arbitrary_values(v in proptest::collection::vec(
        proptest::option::of((".*", any::<u64>())), 0..8,
    )) {
        let off = shared_offloader();
        let out: Nested = off.call("__offload_echo", &(v.clone(),)).unwrap();
        prop_assert_eq!(out, v);
    }
}

fn shared_offloader() -> &'static Offloader {
    static OFF: OnceLock<Offloader> = OnceLock::new();
    OFF.get_or_init(|| offloader(InstancePolicy::PerCall))
}
