use offload::offload;

#[offload]
fn increment(value: Option<usize>) -> u64 {
    value.unwrap_or_default() as u64 + 1
}

fn main() {}
