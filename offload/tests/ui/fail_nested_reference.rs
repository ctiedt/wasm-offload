use offload::offload;

#[offload]
fn count(values: Vec<&'static str>) -> u32 {
    values.len() as u32
}

fn main() {}
