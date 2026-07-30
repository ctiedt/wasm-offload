use offload::offload;

#[offload(deny_floats)]
fn distance(value: f32) -> f32 {
    value.abs()
}

fn main() {}
