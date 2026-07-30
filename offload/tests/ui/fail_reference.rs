use offload::offload;

#[offload]
fn length(value: &str) -> u32 {
    value.len() as u32
}

fn main() {}
