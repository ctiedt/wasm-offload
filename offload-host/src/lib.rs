mod offloader;
mod wasmtime_target;

use std::sync::OnceLock;

pub use offload_core::{InstancePolicy, OffloadError, OffloadTarget};
pub use offloader::{Offloader, OffloaderBuilder, WasiConfig};
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

#[doc(hidden)]
pub fn ensure_copy_back_len(
    argument: usize,
    host: usize,
    guest: usize,
) -> Result<(), OffloadError> {
    if host == guest {
        Ok(())
    } else {
        Err(OffloadError::Runtime(anyhow::anyhow!(
            "copy-back length mismatch for mutable argument {argument}: host length {host}, guest length {guest}"
        )))
    }
}

#[doc(hidden)]
pub fn copy_back_slice<T>(destination: &mut [T], source: Vec<T>) {
    assert_eq!(destination.len(), source.len());
    for (destination, source) in destination.iter_mut().zip(source) {
        *destination = source;
    }
}

#[doc(hidden)]
pub fn copy_back_str(destination: &mut str, source: String) {
    assert_eq!(destination.len(), source.len());
    // this is an unsafe operation because Rust strings have to be valid utf-8, but the source is
    // always a `String` in our case, so it should not cause problems
    unsafe {
        destination
            .as_bytes_mut()
            .copy_from_slice(source.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_back_slice_moves_non_clone_values() {
        #[derive(Debug, PartialEq, Eq)]
        struct Value(u32);

        let mut destination = [Value(1), Value(2)];
        copy_back_slice(&mut destination, vec![Value(3), Value(4)]);
        assert_eq!(destination, [Value(3), Value(4)]);
    }

    #[test]
    fn copy_back_str_can_change_utf8_character_boundaries() {
        let mut destination = String::from("éa");
        let source = String::from("aé");
        ensure_copy_back_len(0, destination.len(), source.len()).unwrap();
        copy_back_str(destination.as_mut_str(), source);
        assert_eq!(destination, "aé");
    }

    #[test]
    fn copy_back_rejects_length_changes() {
        let error = ensure_copy_back_len(2, 3, 4).unwrap_err();
        assert!(error.to_string().contains("mutable argument 2"));
    }
}
