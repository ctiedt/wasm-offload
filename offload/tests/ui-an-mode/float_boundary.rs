use offload::offload;

#[offload]
fn distance(value: f64) -> f64 {
    value.abs()
}

fn main() {}
