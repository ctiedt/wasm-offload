use wasm_offload_procmacro::offload;

#[offload]
fn add(a: u32, b: u32) -> u32 {
    a + b
}

fn main() {}
