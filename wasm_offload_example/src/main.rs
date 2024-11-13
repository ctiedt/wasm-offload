mod types;
use types::Point;
use wasm_offload::{offload, OffloadTarget};

wasm_offload_wasmtime::init_offload!();

#[offload(types = "types.rs")]
fn dist(p1: Point, p2: Point) -> f32 {
    ((p1.x as f32 - p2.x as f32).powf(2.0) + (p1.y as f32 - p2.y as f32).powf(2.0)).sqrt()
}

fn main() {
    println!("Hello, world!");
    let p1 = Point { x: 0, y: 0 };
    let p2 = Point { x: 1, y: 1 };
    println!("{:?}", dist(p1, p2));
}
