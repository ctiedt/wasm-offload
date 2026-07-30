mod offloader;
mod target;
mod wasmtime_target;

use std::sync::OnceLock;

pub use offload_core::OffloadError;
pub use offloader::{InstancePolicy, Offloader, OffloaderBuilder, WasiConfig};
pub use target::OffloadTarget;
pub use wasmtime_target::WasmtimeTarget;

static GLOBAL: OnceLock<Offloader> = OnceLock::new();

pub fn init(offloader: Offloader) -> Result<(), OffloadError> {
    GLOBAL
        .set(offloader)
        .map_err(|_| OffloadError::AlreadyInitialized)
}

pub fn global() -> Result<&'static Offloader, OffloadError> {
    GLOBAL.get().ok_or(OffloadError::Uninitialized)
}

pub fn call<A, R>(export: &str, args: &A) -> Result<R, OffloadError>
where
    A: serde::Serialize,
    R: serde::de::DeserializeOwned,
{
    global()?.call(export, args)
}

pub fn call_checked<A, R>(export: &str, sig: u64, args: &A) -> Result<R, OffloadError>
where
    A: serde::Serialize,
    R: serde::de::DeserializeOwned,
{
    global()?.call_checked(export, sig, args)
}
