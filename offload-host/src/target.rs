use offload_core::OffloadError;

use crate::offloader::InstancePolicy;

pub trait OffloadTarget: Send + Sync + 'static {
    fn prepare(&mut self, module: &[u8]) -> Result<(), OffloadError>;

    fn call_raw(
        &self,
        export: &str,
        args: &[u8],
        policy: InstancePolicy,
    ) -> Result<Vec<u8>, OffloadError>;

    fn abi_version(&self) -> Result<u32, OffloadError>;
}
