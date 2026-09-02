use offload::offload;

#[offload]
fn length(value: &'static str) -> u32 {
    value.len() as u32
}

fn main() {}
