use offload::{AnCompatible, offload};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, AnCompatible)]
struct Wrapper<T> {
    value: Option<T>,
}

#[derive(Serialize, Deserialize, AnCompatible)]
enum Message {
    Empty,
    Values(Vec<(String, u64)>),
}

#[offload(deny_floats)]
fn valid(value: Wrapper<u32>, message: Message) -> (Wrapper<u32>, Message) {
    (value, message)
}

#[offload(try, export = "explicit-name")]
fn fallible_signature(value: i32) -> i32 {
    if value < 0 {
        return 0;
    }
    value
}

fn main() {
    let _: fn(Wrapper<u32>, Message) -> (Wrapper<u32>, Message) = valid;
    let _: fn(i32) -> Result<i32, offload::OffloadError> = fallible_signature;
}
