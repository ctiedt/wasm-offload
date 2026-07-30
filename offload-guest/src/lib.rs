#[cfg(target_arch = "wasm32")]
mod guest;

#[cfg(target_arch = "wasm32")]
pub use guest::entry;

#[cfg(target_arch = "wasm32")]
mod exports {

    const fn str_eq(a: &str, b: &str) -> bool {
        let (a, b) = (a.as_bytes(), b.as_bytes());
        if a.len() != b.len() {
            return false;
        }
        let mut index = 0;
        while index < a.len() {
            if a[index] != b[index] {
                return false;
            }
            index += 1;
        }
        true
    }
    const _: () = assert!(str_eq(offload_core::ALLOC_EXPORT, "__offload_alloc"));
    const _: () = assert!(str_eq(offload_core::FREE_EXPORT, "__offload_free"));
    const _: () = assert!(str_eq(
        offload_core::ABI_VERSION_EXPORT,
        "__offload_abi_version"
    ));

    #[unsafe(no_mangle)]
    pub extern "C" fn __offload_alloc(len: u32) -> u32 {
        super::guest::alloc_bytes(len as usize) as u32
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn __offload_free(ptr: u32, len: u32) {
        unsafe { super::guest::free_bytes(ptr as *mut u8, len as usize) }
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn __offload_abi_version() -> u32 {
        offload_core::ABI_VERSION
    }
}
