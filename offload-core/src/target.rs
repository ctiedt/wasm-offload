use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::OffloadError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstancePolicy {
    #[default]
    PerCall,
    Shared,
}

pub trait OffloadTarget: Send + Sync + 'static {
    fn prepare(&mut self, module: &[u8], policy: InstancePolicy) -> Result<(), OffloadError>;

    fn call_raw(&self, export: &str, args: &[u8]) -> Result<Vec<u8>, OffloadError>;

    fn abi_version(&self) -> Result<u32, OffloadError>;
}
