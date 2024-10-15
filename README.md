# WASM Offload

This project provides a set of crates to offload Rust functions to a WebAssembly
runtime. Just annotate any functions you want to offload with `#[offload]` and
define your `OffloadTarget` and your code will seamlessly run in WebAssembly.

This project is still very early development, so very little works yet.
