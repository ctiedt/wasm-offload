use offload_core::{InstancePolicy, OffloadError};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 2;
pub const HELLO_MAGIC: u32 = u32::from_be_bytes(*b"OFLD");
pub const HELLO_ACK_MAGIC: u32 = u32::from_be_bytes(*b"OFLA");
pub const MAX_FRAME_LEN: u32 = 64 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Hello {
        magic: u32,
        protocol_version: u32,
    },
    Prepare {
        module: Vec<u8>,
        policy: InstancePolicy,
    },
    AbiVersion,
    Call {
        export: String,
        args: Vec<u8>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Hello { magic: u32, protocol_version: u32 },
    Prepared,
    AbiVersion(u32),
    Called(Vec<u8>),
    Err(WireError),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum WireError {
    Encode(postcard::Error),
    Decode(postcard::Error),
    GuestTrap(String),
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
    Runtime(String),
    Protocol(String),
}

pub fn to_wire(error: &OffloadError) -> WireError {
    match error {
        OffloadError::Encode(error) => WireError::Encode(error.clone()),
        OffloadError::Decode(error) => WireError::Decode(error.clone()),
        OffloadError::GuestTrap(error) => WireError::GuestTrap(format!("{error:#}")),
        OffloadError::MissingExport(export) => WireError::MissingExport(export.clone()),
        OffloadError::SignatureMismatch {
            export,
            expected,
            found,
        } => WireError::SignatureMismatch {
            export: export.clone(),
            expected: *expected,
            found: *found,
        },
        OffloadError::AbiVersion { host, guest } => WireError::AbiVersion {
            host: *host,
            guest: *guest,
        },
        OffloadError::Runtime(error) => WireError::Runtime(format!("{error:#}")),
        other => WireError::Runtime(other.to_string()),
    }
}

impl From<WireError> for OffloadError {
    fn from(error: WireError) -> Self {
        match error {
            WireError::Encode(error) => Self::Encode(error),
            WireError::Decode(error) => Self::Decode(error),
            WireError::GuestTrap(error) => Self::GuestTrap(anyhow::anyhow!(error)),
            WireError::MissingExport(export) => Self::MissingExport(export),
            WireError::SignatureMismatch {
                export,
                expected,
                found,
            } => Self::SignatureMismatch {
                export,
                expected,
                found,
            },
            WireError::AbiVersion { host, guest } => Self::AbiVersion { host, guest },
            WireError::Runtime(error) => Self::Runtime(anyhow::anyhow!(error)),
            WireError::Protocol(error) => Self::Transport(anyhow::anyhow!(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_encoding_is_stable() {
        let encoded = postcard::to_allocvec(&Request::Hello {
            magic: HELLO_MAGIC,
            protocol_version: PROTOCOL_VERSION,
        })
        .unwrap();
        assert_eq!(encoded, [0, 0xc4, 0x98, 0x99, 0xfa, 0x04, 2]);
    }

    #[test]
    fn call_encoding_is_stable() {
        let encoded = postcard::to_allocvec(&Request::Call {
            export: "abc".into(),
            args: vec![0xaa, 0xbb],
        })
        .unwrap();
        assert_eq!(encoded, [3, 3, b'a', b'b', b'c', 2, 0xaa, 0xbb]);
    }

    #[test]
    fn prepare_encoding_is_stable() {
        let encoded = postcard::to_allocvec(&Request::Prepare {
            module: vec![0xaa, 0xbb],
            policy: InstancePolicy::Shared,
        })
        .unwrap();
        assert_eq!(encoded, [1, 2, 0xaa, 0xbb, 1]);
    }

    #[test]
    fn structured_errors_map_in_both_directions() {
        let wire = to_wire(&OffloadError::SignatureMismatch {
            export: "__offload_f".into(),
            expected: 7,
            found: 8,
        });
        assert!(matches!(
            wire,
            WireError::SignatureMismatch {
                ref export,
                expected: 7,
                found: 8,
            } if export == "__offload_f"
        ));

        let host = OffloadError::from(WireError::AbiVersion { host: 1, guest: 2 });
        assert!(matches!(
            host,
            OffloadError::AbiVersion { host: 1, guest: 2 }
        ));

        let host = OffloadError::from(WireError::MissingExport("missing".into()));
        assert!(matches!(
            host,
            OffloadError::MissingExport(ref export) if export == "missing"
        ));
    }

    #[test]
    fn postcard_and_string_errors_map_in_both_directions() {
        let wire = to_wire(&OffloadError::Encode(
            postcard::Error::SerializeSeqLengthUnknown,
        ));
        assert!(matches!(
            wire,
            WireError::Encode(postcard::Error::SerializeSeqLengthUnknown)
        ));

        let host = OffloadError::from(WireError::Decode(postcard::Error::DeserializeUnexpectedEnd));
        assert!(matches!(
            host,
            OffloadError::Decode(postcard::Error::DeserializeUnexpectedEnd)
        ));

        let host = OffloadError::from(WireError::GuestTrap("trap details".into()));
        assert!(matches!(host, OffloadError::GuestTrap(_)));

        let host = OffloadError::from(WireError::Runtime("runtime details".into()));
        assert!(matches!(host, OffloadError::Runtime(_)));

        let host = OffloadError::from(WireError::Protocol("bad request".into()));
        assert!(matches!(host, OffloadError::Transport(_)));
    }
}
