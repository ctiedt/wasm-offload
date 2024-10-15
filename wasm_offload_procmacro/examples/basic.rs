use wasm_offload_procmacro::offload;

#[offload]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn main() {}
