use mylib::Point;
use std::time::Instant;

fn main() {
    let addr = std::env::var("OFFLOAD_REMOTE_ADDR").unwrap_or_else(|_| {
        eprintln!(
            "OFFLOAD_REMOTE_ADDR is not set"
        );
        std::process::exit(2);
    });
    println!("offloading to {addr}");
    println!("Uploading WASM module...");
    let start = Instant::now();
    offload::init_guest!(artifact = "mylib", target = offload::RemoteTarget::tcp(addr)).unwrap();
    let elapsed = start.elapsed();
    println!("Finished setup! (took {:.2?})", elapsed);
    println!("");

    let distance = mylib::dist(Point { x: 0.0, y: 0.0 }, Point { x: 3.0, y: 4.0 });
    println!("distance={distance}");

    let distance = mylib::dist(Point { x: 2.0, y: 1.0 }, Point { x: 3.0, y: 4.0 });
    println!("distance={distance}");
}
