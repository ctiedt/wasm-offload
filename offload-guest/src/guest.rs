use core::alloc::Layout;

use offload_core::{BUFFER_ALIGN, pack_ret};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub(crate) fn alloc_bytes(len: usize) -> *mut u8 {
    if len == 0 {
        return BUFFER_ALIGN as *mut u8;
    }
    let layout = Layout::from_size_align(len, BUFFER_ALIGN).expect("offload: alloc layout");
    let ptr = unsafe { std::alloc::alloc(layout) };
    if ptr.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    ptr
}

pub(crate) unsafe fn free_bytes(ptr: *mut u8, len: usize) {
    if len == 0 {
        return;
    }
    let layout = Layout::from_size_align(len, BUFFER_ALIGN).expect("offload: free layout");
    unsafe { std::alloc::dealloc(ptr, layout) }
}

pub fn entry<Args, R>(ptr: u32, len: u32, f: impl FnOnce(Args) -> R) -> u64
where
    Args: DeserializeOwned,
    R: Serialize,
{
    let input = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let (args, rest): (Args, _) =
        postcard::take_from_bytes(input).expect("offload: argument decode");
    assert!(
        rest.is_empty(),
        "offload: argument decode left {} trailing bytes (host/guest signature mismatch?)",
        rest.len()
    );
    let ret = f(args);
    let out = postcard::to_allocvec(&ret).expect("offload: return encode");
    pack_and_leak(&out)
}

fn pack_and_leak(bytes: &[u8]) -> u64 {
    let len = bytes.len();
    let ptr = alloc_bytes(len);
    if len > 0 {
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, len) };
    }
    pack_ret(ptr as u32, len as u32)
}
