fn main() {
    offload::init_guest!(artifact = "mylib").unwrap();
    let runtime_panic = mylib::intentional_panic().unwrap_err();
    println!("{runtime_panic}");
}
