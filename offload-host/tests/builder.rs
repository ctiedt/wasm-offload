use offload_core::{ABI_VERSION, MANIFEST_SECTION, ManifestRecord, OffloadError};
use offload_host::{InstancePolicy, OffloadTarget, Offloader};

struct FakeTarget {
    version: u32,
}

impl OffloadTarget for FakeTarget {
    fn prepare(&mut self, _module: &[u8]) -> Result<(), OffloadError> {
        Ok(())
    }

    fn call_raw(
        &self,
        _export: &str,
        _args: &[u8],
        _policy: InstancePolicy,
    ) -> Result<Vec<u8>, OffloadError> {
        unreachable!()
    }

    fn abi_version(&self) -> Result<u32, OffloadError> {
        Ok(self.version)
    }
}

#[test]
fn coarse_abi_handshake_rejects_mismatch() {
    let error = Offloader::builder(b"\0asm\x01\0\0\0")
        .target(FakeTarget {
            version: ABI_VERSION + 1,
        })
        .build()
        .err()
        .expect("ABI mismatch");
    assert!(matches!(
        error,
        OffloadError::AbiVersion {
            host: ABI_VERSION,
            guest
        } if guest == ABI_VERSION + 1
    ));
}

#[test]
fn manifest_abi_mismatch_fails_before_runtime_preparation() {
    let record = ManifestRecord::new("__offload_value", ABI_VERSION + 1, 42);
    let module =
        module_with_custom_section(MANIFEST_SECTION, &postcard::to_allocvec(&record).unwrap());
    let error = Offloader::builder(&module)
        .target(FakeTarget {
            version: ABI_VERSION,
        })
        .build()
        .err()
        .expect("manifest ABI mismatch");
    assert!(matches!(error, OffloadError::AbiVersion { .. }));
}

#[test]
fn malformed_manifest_is_a_runtime_error() {
    let module = module_with_custom_section(MANIFEST_SECTION, &[0xff]);
    let error = Offloader::builder(&module)
        .target(FakeTarget {
            version: ABI_VERSION,
        })
        .build()
        .err()
        .expect("malformed manifest");
    assert!(matches!(error, OffloadError::Runtime(_)));
}

fn module_with_custom_section(name: &str, data: &[u8]) -> Vec<u8> {
    let mut payload = Vec::new();
    push_uleb(name.len() as u32, &mut payload);
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(data);

    let mut module = b"\0asm\x01\0\0\0".to_vec();
    module.push(0);
    push_uleb(payload.len() as u32, &mut module);
    module.extend(payload);
    module
}

fn push_uleb(mut value: u32, output: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        output.push(byte);
        if value == 0 {
            break;
        }
    }
}
