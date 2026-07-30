use offload::offload;

#[offload]
async fn compute(value: u32) -> u32 {
    value
}

fn main() {}
