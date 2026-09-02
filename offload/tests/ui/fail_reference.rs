use offload::offload;

#[offload]
fn identity(value: &String) -> &String {
    value
}

fn main() {}
