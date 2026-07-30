pub use offload_core::{ABI_VERSION, AnCompatible, BoundaryCompatible, OffloadError};
pub use offload_macros::{AnCompatible, include_guest, init_guest, offload};

#[cfg(not(target_arch = "wasm32"))]
pub use offload_host::{
    InstancePolicy, OffloadTarget, Offloader, OffloaderBuilder, WasiConfig, WasmtimeTarget, global,
    init,
};

#[doc(hidden)]
pub mod __private {
    #[cfg(target_arch = "wasm32")]
    pub use offload_guest as guest;

    #[cfg(not(target_arch = "wasm32"))]
    pub use offload_host as host;
}
