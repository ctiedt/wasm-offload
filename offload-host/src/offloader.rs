use std::collections::HashMap;
use std::sync::Mutex;

use offload_core::{ABI_VERSION, MANIFEST_SECTION, ManifestRecord, OffloadError};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::target::OffloadTarget;
use crate::wasmtime_target::WasmtimeTarget;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InstancePolicy {
    #[default]
    PerCall,
    Shared,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WasiConfig {
    pub(crate) inherit_stdin: bool,
    pub(crate) inherit_stdout: bool,
    pub(crate) inherit_stderr: bool,
}

impl WasiConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn inherit_stdin(mut self, yes: bool) -> Self {
        self.inherit_stdin = yes;
        self
    }

    pub fn inherit_stdout(mut self, yes: bool) -> Self {
        self.inherit_stdout = yes;
        self
    }

    pub fn inherit_stderr(mut self, yes: bool) -> Self {
        self.inherit_stderr = yes;
        self
    }

    pub fn inherit_output(self) -> Self {
        self.inherit_stdout(true).inherit_stderr(true)
    }
}

pub struct Offloader {
    target: Box<dyn OffloadTarget>,
    policy: InstancePolicy,
    manifest: HashMap<String, u64>,
    checked: Mutex<HashMap<String, u64>>,
}

impl Offloader {
    pub fn builder(module_bytes: &[u8]) -> OffloaderBuilder<'_> {
        OffloaderBuilder {
            module_bytes,
            policy: InstancePolicy::default(),
            wasi: WasiConfig::default(),
            pooling_allocator: false,
            target: None,
        }
    }

    pub fn call<A, R>(&self, export: &str, args: &A) -> Result<R, OffloadError>
    where
        A: Serialize,
        R: DeserializeOwned,
    {
        let encoded = postcard::to_allocvec(args).map_err(OffloadError::Encode)?;
        let ret = self.target.call_raw(export, &encoded, self.policy)?;
        postcard::from_bytes(&ret).map_err(OffloadError::Decode)
    }

    pub fn call_checked<A, R>(&self, export: &str, sig: u64, args: &A) -> Result<R, OffloadError>
    where
        A: Serialize,
        R: DeserializeOwned,
    {
        {
            let mut checked = self
                .checked
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if checked.get(export) != Some(&sig) {
                let found = self
                    .manifest
                    .get(export)
                    .copied()
                    .ok_or_else(|| OffloadError::MissingExport(export.to_owned()))?;
                if found != sig {
                    return Err(OffloadError::SignatureMismatch {
                        export: export.to_owned(),
                        expected: sig,
                        found,
                    });
                }
                checked.insert(export.to_owned(), sig);
            }
        }
        self.call(export, args)
    }
}

pub struct OffloaderBuilder<'a> {
    module_bytes: &'a [u8],
    policy: InstancePolicy,
    wasi: WasiConfig,
    pooling_allocator: bool,
    target: Option<Box<dyn OffloadTarget>>,
}

impl OffloaderBuilder<'_> {
    pub fn instance_policy(mut self, policy: InstancePolicy) -> Self {
        self.policy = policy;
        self
    }

    pub fn wasi(mut self, config: WasiConfig) -> Self {
        self.wasi = config;
        self
    }

    pub fn pooling_allocator(mut self, enabled: bool) -> Self {
        self.pooling_allocator = enabled;
        self
    }

    pub fn target(mut self, target: impl OffloadTarget) -> Self {
        self.target = Some(Box::new(target));
        self
    }

    pub fn build(self) -> Result<Offloader, OffloadError> {
        let mut target = match self.target {
            Some(target) => target,
            None if self.pooling_allocator => Box::new(WasmtimeTarget::with_pooling(self.wasi)?),
            None => Box::new(WasmtimeTarget::new(self.wasi)),
        };
        target.prepare(self.module_bytes)?;
        let guest = target.abi_version()?;
        if guest != ABI_VERSION {
            return Err(OffloadError::AbiVersion {
                host: ABI_VERSION,
                guest,
            });
        }
        let manifest = parse_manifest(self.module_bytes)?;
        Ok(Offloader {
            target,
            policy: self.policy,
            manifest,
            checked: Mutex::new(HashMap::new()),
        })
    }
}

fn parse_manifest(module: &[u8]) -> Result<HashMap<String, u64>, OffloadError> {
    let mut records = HashMap::new();
    for payload in wasmparser::Parser::new(0).parse_all(module) {
        let payload = payload.map_err(|error| OffloadError::Runtime(error.into()))?;
        let wasmparser::Payload::CustomSection(section) = payload else {
            continue;
        };
        if section.name() != MANIFEST_SECTION {
            continue;
        }

        let mut bytes = section.data();
        while !bytes.is_empty() {
            let (record, rest): (ManifestRecord, _) =
                postcard::take_from_bytes(bytes).map_err(|error| {
                    OffloadError::Runtime(anyhow::anyhow!(
                        "malformed `{MANIFEST_SECTION}` custom section: {error}"
                    ))
                })?;
            if record.abi_version != ABI_VERSION {
                return Err(OffloadError::AbiVersion {
                    host: ABI_VERSION,
                    guest: record.abi_version,
                });
            }
            if let Some(previous) = records.insert(record.export_name.clone(), record.sig_hash)
                && previous != record.sig_hash
            {
                return Err(OffloadError::Runtime(anyhow::anyhow!(
                    "conflicting manifest records for `{}`",
                    record.export_name
                )));
            }
            bytes = rest;
        }
    }
    Ok(records)
}
