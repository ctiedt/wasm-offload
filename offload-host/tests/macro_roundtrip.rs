use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use offload_core::{MANIFEST_SECTION, ManifestRecord, OffloadError};
use offload_host::Offloader;

fn guest_bytes() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_dir.parent().unwrap();
        let guest_dir = workspace_root.join("tests/guests/macro-guest");
        let target_dir = workspace_root.join("target/test-macro-guest");
        let status = Command::new(env!("CARGO"))
            .current_dir(&guest_dir)
            .args(["build", "--locked", "--target", "wasm32-wasip1"])
            .arg("--target-dir")
            .arg(&target_dir)
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .status()
            .expect("failed to spawn cargo for macro guest");
        assert!(status.success(), "macro guest build failed");
        let wasm = target_dir.join("wasm32-wasip1/debug/offload_macro_test_guest.wasm");
        std::fs::read(&wasm).unwrap_or_else(|error| panic!("read {}: {error}", wasm.display()))
    })
}

fn manifest() -> HashMap<String, u64> {
    let mut records = HashMap::new();
    for payload in wasmparser::Parser::new(0).parse_all(guest_bytes()) {
        if let wasmparser::Payload::CustomSection(section) = payload.unwrap() {
            if section.name() != MANIFEST_SECTION {
                continue;
            }
            let mut bytes = section.data();
            while !bytes.is_empty() {
                let (record, rest): (ManifestRecord, _) = postcard::take_from_bytes(bytes).unwrap();
                records.insert(record.export_name, record.sig_hash);
                bytes = rest;
            }
        }
    }
    records
}

#[test]
fn generated_exports_roundtrip_and_nested_calls_stay_in_guest() {
    let offloader = Offloader::builder(guest_bytes()).build().unwrap();
    let hashes = manifest();
    assert_eq!(hashes.len(), 7);

    let add: i64 = offloader
        .call_checked("__offload_add", hashes["__offload_add"], &(40i64, 2i64))
        .unwrap();
    assert_eq!(add, 42);

    let reversed: Vec<u32> = offloader
        .call_checked(
            "reverse-values",
            hashes["reverse-values"],
            &(vec![1u32, 2, 3],),
        )
        .unwrap();
    assert_eq!(reversed, [3, 2, 1]);

    let incremented: i32 = offloader
        .call_checked(
            "__offload_checked_increment",
            hashes["__offload_checked_increment"],
            &(i32::MAX,),
        )
        .unwrap();
    assert_eq!(incremented, i32::MIN);

    let nested: i32 = offloader
        .call_checked("__offload_nested", hashes["__offload_nested"], &(20i32,))
        .unwrap();
    assert_eq!(nested, 41);

    let destructured: i32 = offloader
        .call_checked(
            "__offload_destructured",
            hashes["__offload_destructured"],
            &((9i32, 4i32),),
        )
        .unwrap();
    assert_eq!(destructured, 5);
}

#[test]
fn stale_signature_is_rejected_before_dispatch() {
    let offloader = Offloader::builder(guest_bytes()).build().unwrap();
    let actual = manifest()["__offload_add"];
    let error = offloader
        .call_checked::<_, i64>("__offload_add", actual ^ 1, &(1i64, 2i64))
        .unwrap_err();
    match error {
        OffloadError::SignatureMismatch {
            export,
            expected,
            found,
        } => {
            assert_eq!(export, "__offload_add");
            assert_eq!(expected, actual ^ 1);
            assert_eq!(found, actual);
        }
        other => panic!("expected SignatureMismatch, got {other}"),
    }
}

#[test]
fn duplicate_default_exports_fail_the_wasm_build() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap();
    let guest_dir = workspace_root.join("tests/guests/duplicate-guest");
    let target_dir = workspace_root.join("target/test-duplicate-guest");
    let output = Command::new(env!("CARGO"))
        .current_dir(&guest_dir)
        .args(["build", "--locked", "--target", "wasm32-wasip1"])
        .arg("--target-dir")
        .arg(&target_dir)
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .output()
        .expect("failed to spawn cargo for duplicate guest");
    assert!(
        !output.status.success(),
        "duplicate exports unexpectedly linked"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("__offload_value")
            && (stderr.contains("defined multiple times") || stderr.contains("already defined")),
        "unexpected duplicate-export diagnostic:\n{stderr}"
    );
}
