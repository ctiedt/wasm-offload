use offload::offload;

#[offload]
pub fn add(a: i64, b: i64) -> i64 {
    a + b
}

#[offload(try)]
pub fn intentional_panic() {
    panic!("guest trap");
}
