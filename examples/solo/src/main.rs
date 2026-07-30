use offload::offload;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct Point {
    x: f32,
    y: f32,
}

#[offload]
fn distance(a: Point, b: Point) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    (dx * dx + dy * dy).sqrt()
}

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    offload::init_guest!()?;

    let distance = distance(Point { x: 0.0, y: 0.0 }, Point { x: 1.0, y: 1.0 });
    println!("distance={distance}");
    Ok(())
}
