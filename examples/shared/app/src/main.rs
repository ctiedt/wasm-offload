fn main() {
    offload::init_guest!(
        artifact = "mylib",
        instance_policy = offload::InstancePolicy::Shared,
        wasi = offload::WasiConfig::new().inherit_output(),
    ).unwrap();

    let calls = [
        mylib::guest_call_count(),
        mylib::guest_call_count(),
        mylib::guest_call_count(),
    ];
    println!("host: guest call counts = {calls:?}");
    assert_eq!(calls, [1, 2, 3]);
}
