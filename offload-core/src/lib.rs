#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod error;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

pub use error::OffloadError;

pub const ABI_VERSION: u32 = 1;

pub const EXPORT_PREFIX: &str = "__offload_";

pub const ALLOC_EXPORT: &str = "__offload_alloc";

pub const FREE_EXPORT: &str = "__offload_free";

pub const ABI_VERSION_EXPORT: &str = "__offload_abi_version";

pub const MEMORY_EXPORT: &str = "memory";

pub const MANIFEST_SECTION: &str = "offload-manifest";

pub const GUEST_PATH_ENV: &str = "OFFLOAD_GUEST_PATH";

pub fn guest_path_env_suffix(package: &str) -> String {
    package
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

pub const BUFFER_ALIGN: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestRecord {
    pub export_name: String,
    pub abi_version: u32,
    pub sig_hash: u64,
}

impl ManifestRecord {
    pub fn new(export_name: impl Into<String>, abi_version: u32, sig_hash: u64) -> Self {
        Self {
            export_name: export_name.into(),
            abi_version,
            sig_hash,
        }
    }
}

#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot cross the offload boundary in AN mode",
    note = "floating-point values and pointer-sized integers (`usize`/`isize`) are not portable across the offload boundary; derive `AnCompatible` for structs and enums built from fixed-width types"
)]
pub trait AnCompatible {}

#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot cross the offload boundary in AN mode",
    note = "floating-point values are not representable under AN encoding; derive `AnCompatible` for float-free structs and enums"
)]
pub trait BoundaryCompatible {}

#[cfg(feature = "an-mode")]
impl<T: AnCompatible> BoundaryCompatible for T {}

#[cfg(not(feature = "an-mode"))]
impl<T> BoundaryCompatible for T {}

macro_rules! impl_an_primitive {
    ($($ty:ty),+ $(,)?) => { $(impl AnCompatible for $ty {})+ };
}

impl_an_primitive!(u8, u16, u32, u64, i8, i16, i32, i64, bool, char, String, (),);

impl<T: AnCompatible> AnCompatible for Vec<T> {}
impl<T: AnCompatible> AnCompatible for Option<T> {}
impl<T: AnCompatible> AnCompatible for Box<T> {}
impl<T: AnCompatible, const N: usize> AnCompatible for [T; N] {}
impl<T: AnCompatible, E: AnCompatible> AnCompatible for Result<T, E> {}

macro_rules! impl_an_tuple {
    ($($name:ident),+ $(,)?) => {
        impl<$($name: AnCompatible),+> AnCompatible for ($($name,)+) {}
    };
}

impl_an_tuple!(T0);
impl_an_tuple!(T0, T1);
impl_an_tuple!(T0, T1, T2);
impl_an_tuple!(T0, T1, T2, T3);
impl_an_tuple!(T0, T1, T2, T3, T4);
impl_an_tuple!(T0, T1, T2, T3, T4, T5);
impl_an_tuple!(T0, T1, T2, T3, T4, T5, T6);
impl_an_tuple!(T0, T1, T2, T3, T4, T5, T6, T7);
impl_an_tuple!(T0, T1, T2, T3, T4, T5, T6, T7, T8);
impl_an_tuple!(T0, T1, T2, T3, T4, T5, T6, T7, T8, T9);
impl_an_tuple!(T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10);
impl_an_tuple!(T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11);

pub const fn sig_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut index = 0;
    while index < bytes.len() {
        hash ^= bytes[index] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        index += 1;
    }
    hash
}

#[inline]
pub const fn pack_ret(ptr: u32, len: u32) -> u64 {
    ((ptr as u64) << 32) | len as u64
}

#[inline]
pub const fn unpack_ret(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, packed as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_unpack_roundtrip() {
        for (ptr, len) in [
            (0u32, 0u32),
            (8, 0),
            (0x1000, 1),
            (u32::MAX, u32::MAX),
            (0xdead_beef, 0x1234_5678),
        ] {
            assert_eq!(unpack_ret(pack_ret(ptr, len)), (ptr, len));
        }
    }

    #[test]
    fn pack_layout_is_ptr_high_len_low() {
        assert_eq!(pack_ret(1, 2), (1u64 << 32) | 2);
    }

    fn assert_an<T: AnCompatible>() {}

    #[test]
    fn representative_an_compatible_types_compile() {
        assert_an::<Vec<Option<(String, u64)>>>();
        assert_an::<Box<[Result<i32, char>; 2]>>();
    }

    #[test]
    fn manifest_record_roundtrips_and_is_self_delimiting() {
        let first = ManifestRecord::new("__offload_a", ABI_VERSION, 1);
        let second = ManifestRecord::new("custom", ABI_VERSION, u64::MAX);
        let mut bytes = postcard::to_allocvec(&first).unwrap();
        bytes.extend(postcard::to_allocvec(&second).unwrap());

        let (decoded_first, rest): (ManifestRecord, _) = postcard::take_from_bytes(&bytes).unwrap();
        let (decoded_second, rest): (ManifestRecord, _) = postcard::take_from_bytes(rest).unwrap();
        assert_eq!(decoded_first, first);
        assert_eq!(decoded_second, second);
        assert!(rest.is_empty());
    }
}
