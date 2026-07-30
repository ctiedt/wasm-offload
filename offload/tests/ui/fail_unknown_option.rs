use offload::offload;

#[offload(remote)]
fn value() -> u32 {
    1
}

fn main() {}
