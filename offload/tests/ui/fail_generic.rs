use offload::offload;

#[offload]
fn identity<T>(value: T) -> T {
    value
}

fn main() {}
