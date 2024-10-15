use std::{io::Write, process::Stdio};

use proc_macro::TokenStream;
use quote::quote;
use syn::ItemFn;

extern crate proc_macro;

#[proc_macro_attribute]
pub fn offload(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let prog_input = item.to_string().into_bytes();
    let input = syn::parse_macro_input!(item as ItemFn);
    let mut proc = std::process::Command::new("rustc")
        .args([
            "--crate-type=cdylib",
            "--target=wasm32-wasi",
            "-o",
            "-",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = proc.stdin.take().unwrap();
    std::thread::spawn(move || {
        stdin.write_all(&prog_input).unwrap();
    });
    let wasm_output = proc.wait_with_output().unwrap().stdout;
    let fn_sig = input.sig;
    let fn_name = fn_sig.ident;
    let fn_args = fn_sig.inputs;
    let fn_returns = fn_sig.output;
    let output = quote! {
        pub fn #fn_name(#fn_args) #fn_returns {
            let wasm = vec![#(#wasm_output),*];
        }
    };
    TokenStream::from(output)
}
