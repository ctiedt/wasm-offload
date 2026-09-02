use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use offload::{OffloadError, offload};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Point {
    x: i32,
    y: i32,
}

#[offload]
fn inspect_references(point: &Point, label: &str, values: &[i32]) -> i32 {
    point.x + point.y + label.len() as i32 + values.iter().sum::<i32>()
}

#[offload]
fn mutate_references(point: &mut Point, label: &mut str, values: &mut [i32]) -> i32 {
    point.x += 10;
    point.y -= 5;
    label.make_ascii_uppercase();
    for value in values.iter_mut() {
        *value *= 2;
    }
    point.x + point.y + label.len() as i32 + values.iter().sum::<i32>()
}

#[offload(try)]
fn try_mutate_reference(point: &mut Point) -> i32 {
    point.x += 1;
    point.y += 2;
    point.x + point.y
}

#[offload(try)]
fn mutate_then_panic(point: &mut Point) {
    point.x = i32::MAX;
    panic!("mutation failed")
}

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

#[test]
fn references_use_copy_in_copy_out_semantics() {
    let offloader = offload::Offloader::builder(guest_bytes()).build().unwrap();
    offload::init(offloader).unwrap();

    let mut point = Point { x: 2, y: 3 };
    let observed = inspect_references(&point, "hello", &[1, 2, 3]);
    assert_eq!(observed, 16);
    assert_eq!(point, Point { x: 2, y: 3 });

    let mut label = String::from("héllo");
    let mut values = [1, 2, 3];
    let returned = mutate_references(&mut point, label.as_mut_str(), &mut values);
    assert_eq!(returned, 28);
    assert_eq!(point, Point { x: 12, y: -2 });
    assert_eq!(label, "HéLLO");
    assert_eq!(values, [2, 4, 6]);

    let returned = try_mutate_reference(&mut point).unwrap();
    assert_eq!(returned, 13);
    assert_eq!(point, Point { x: 13, y: 0 });

    let before_trap = point.clone();
    let error = mutate_then_panic(&mut point).unwrap_err();
    assert!(matches!(error, OffloadError::GuestTrap(_)));
    assert_eq!(point, before_trap);
}
