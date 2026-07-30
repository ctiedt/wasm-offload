use offload::offload;

#[cfg(target_arch = "wasm32")]
use std::sync::atomic::{AtomicU32, Ordering};

#[cfg(target_arch = "wasm32")]
static GUEST_CALLS: AtomicU32 = AtomicU32::new(0);

#[offload]
pub fn guest_call_count() -> u32 {
    let call = GUEST_CALLS.fetch_add(1, Ordering::Relaxed) + 1;
    println!(
        "guest: running on {} (shared call #{call})",
        std::env::consts::ARCH
    );
    call
}
