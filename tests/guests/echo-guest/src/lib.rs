
#![cfg(target_arch = "wasm32")]

use offload::__private::guest;

fn echo(v: Vec<Option<(String, u64)>>) -> Vec<Option<(String, u64)>> {
    v
}

fn add(a: i64, b: i64) -> i64 {
    a.wrapping_add(b)
}

fn echo_bytes(bytes: Vec<u8>) -> Vec<u8> {
    bytes
}

fn nothing() {}

fn panics(msg: String) {
    panic!("guest panic: {msg}");
}

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn bump() -> u64 {
    COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

const _: () = {
    #[unsafe(no_mangle)]
    pub extern "C" fn __offload_echo(ptr: u32, len: u32) -> u64 {
        guest::entry(ptr, len, |(v,): (Vec<Option<(String, u64)>>,)| echo(v))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn __offload_add(ptr: u32, len: u32) -> u64 {
        guest::entry(ptr, len, |(a, b): (i64, i64)| add(a, b))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn __offload_echo_bytes(ptr: u32, len: u32) -> u64 {
        guest::entry(ptr, len, |(bytes,): (Vec<u8>,)| echo_bytes(bytes))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn __offload_nothing(ptr: u32, len: u32) -> u64 {
        guest::entry(ptr, len, |(): ()| nothing())
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn __offload_panics(ptr: u32, len: u32) -> u64 {
        guest::entry(ptr, len, |(msg,): (String,)| panics(msg))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn __offload_bump(ptr: u32, len: u32) -> u64 {
        guest::entry(ptr, len, |(): ()| bump())
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn __offload_invalid_return(_ptr: u32, _len: u32) -> u64 {
        (u64::from(u32::MAX) << 32) | u64::from(u32::MAX)
    }
};
