use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use wasm_offload::offload;
use wasm_offload::OffloadTarget;
use wasm_offload::Val;
use wasm_offload_wasmtime::{init_offload, WasmtimeOffload};

init_offload!();

#[offload]
fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[offload]
fn div(a: i32, b: i32) -> Option<i32> {
    if b == 0 {
        None
    } else {
        Some(a / b)
    }
}

#[offload]
fn say_hello() {
    println!("Hello from the WASM side");
}

fn main() {
    println!("{:?}", add(1, 2));
    println!("{:?}", div(4, 2));
    println!("{:?}", div(4, 0));
    say_hello().unwrap();
}
