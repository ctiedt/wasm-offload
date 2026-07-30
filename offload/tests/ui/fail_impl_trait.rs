use offload::offload;

#[offload]
fn make_value() -> impl Iterator<Item = u32> {
    0..3
}

fn main() {}
