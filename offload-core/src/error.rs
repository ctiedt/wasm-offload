use alloc::string::String;
use core::fmt;

#[derive(Debug)]
#[non_exhaustive]
pub enum OffloadError {
    Uninitialized,
    AlreadyInitialized,
    Encode(postcard::Error),
    Decode(postcard::Error),
    #[cfg(feature = "std")]
    GuestTrap(anyhow::Error),
    MissingExport(String),
    SignatureMismatch {
        export: String,
        expected: u64,
        found: u64,
    },
    AbiVersion {
        host: u32,
        guest: u32,
    },
    #[cfg(feature = "std")]
    Runtime(anyhow::Error),
}

impl fmt::Display for OffloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uninitialized => {
                write!(f, "no global offloader: call offload::init(..) first")
            }
            Self::AlreadyInitialized => {
                write!(f, "offload::init(..) called twice")
            }
            Self::Encode(e) => write!(f, "failed to encode arguments: {e}"),
            Self::Decode(e) => write!(f, "failed to decode return value: {e}"),
            #[cfg(feature = "std")]
            Self::GuestTrap(e) => write!(f, "guest trapped: {e}"),
            Self::MissingExport(name) => {
                write!(f, "guest module is missing export `{name}`")
            }
            Self::SignatureMismatch {
                export,
                expected,
                found,
            } => write!(
                f,
                "signature hash mismatch for `{export}` (host {expected:#018x}, \
                 guest {found:#018x}): the guest artifact is stale or foreign"
            ),
            Self::AbiVersion { host, guest } => write!(
                f,
                "ABI version mismatch: host speaks v{host}, guest artifact is v{guest}"
            ),
            #[cfg(feature = "std")]
            Self::Runtime(e) => write!(f, "offload runtime error: {e}"),
        }
    }
}

impl core::error::Error for OffloadError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Encode(e) | Self::Decode(e) => Some(e),
            #[cfg(feature = "std")]
            Self::GuestTrap(e) | Self::Runtime(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}
