use std::sync::Mutex;

use offload_core::{
    ABI_VERSION_EXPORT, ALLOC_EXPORT, FREE_EXPORT, MEMORY_EXPORT, OffloadError, unpack_ret,
};
use wasmtime::{
    Config, Engine, Instance, InstanceAllocationStrategy, InstancePre, Linker, Memory, Module,
    Store, TypedFunc, WasmParams, WasmResults,
};
use wasmtime_wasi::p1::{self as preview1, WasiP1Ctx};

use crate::offloader::{InstancePolicy, WasiConfig};
use crate::target::OffloadTarget;

const WASI_INITIALIZE_EXPORT: &str = "_initialize";

pub struct WasmtimeTarget {
    engine: Engine,
    wasi: WasiConfig,
    prepared: Option<InstancePre<WasiP1Ctx>>,
    shared: Mutex<Option<SharedInstance>>,
}

struct SharedInstance {
    store: Store<WasiP1Ctx>,
    instance: Instance,
    memory: Memory,
    alloc: TypedFunc<u32, u32>,
    free: TypedFunc<(u32, u32), ()>,
}

impl WasmtimeTarget {
    pub fn new(wasi: WasiConfig) -> Self {
        Self::with_engine(Engine::default(), wasi)
    }

    pub fn with_pooling(wasi: WasiConfig) -> Result<Self, OffloadError> {
        let mut config = Config::new();
        config.allocation_strategy(InstanceAllocationStrategy::pooling());
        let engine = Engine::new(&config).map_err(|error| OffloadError::Runtime(error.into()))?;
        Ok(Self::with_engine(engine, wasi))
    }

    fn with_engine(engine: Engine, wasi: WasiConfig) -> Self {
        Self {
            engine,
            wasi,
            prepared: None,
            shared: Mutex::new(None),
        }
    }

    fn instance_pre(&self) -> Result<&InstancePre<WasiP1Ctx>, OffloadError> {
        self.prepared
            .as_ref()
            .ok_or_else(|| OffloadError::Runtime(anyhow::anyhow!("target not prepared")))
    }

    fn new_store(&self) -> Store<WasiP1Ctx> {
        let mut builder = wasmtime_wasi::WasiCtxBuilder::new();
        if self.wasi.inherit_stdin {
            builder.inherit_stdin();
        }
        if self.wasi.inherit_stdout {
            builder.inherit_stdout();
        }
        if self.wasi.inherit_stderr {
            builder.inherit_stderr();
        }
        Store::new(&self.engine, builder.build_p1())
    }

    fn instantiate(&self) -> Result<SharedInstance, OffloadError> {
        let mut store = self.new_store();
        let instance = self
            .instance_pre()?
            .instantiate(&mut store)
            .map_err(|e| OffloadError::Runtime(e.into()))?;

        if let Some(init) = instance.get_func(&mut store, WASI_INITIALIZE_EXPORT) {
            let init: TypedFunc<(), ()> = init
                .typed(&store)
                .map_err(|e| OffloadError::Runtime(e.into()))?;
            init.call(&mut store, ())
                .map_err(|e| OffloadError::GuestTrap(e.into()))?;
        }

        let memory = instance
            .get_memory(&mut store, MEMORY_EXPORT)
            .ok_or_else(|| OffloadError::MissingExport(MEMORY_EXPORT.into()))?;
        let alloc = typed_func(&mut store, &instance, ALLOC_EXPORT)?;
        let free = typed_func(&mut store, &instance, FREE_EXPORT)?;
        Ok(SharedInstance {
            store,
            instance,
            memory,
            alloc,
            free,
        })
    }
}

impl OffloadTarget for WasmtimeTarget {
    fn prepare(&mut self, module: &[u8]) -> Result<(), OffloadError> {
        let module =
            Module::new(&self.engine, module).map_err(|e| OffloadError::Runtime(e.into()))?;
        let mut linker: Linker<WasiP1Ctx> = Linker::new(&self.engine);
        preview1::add_to_linker_sync(&mut linker, |ctx| ctx)
            .map_err(|e| OffloadError::Runtime(e.into()))?;
        let pre = linker
            .instantiate_pre(&module)
            .map_err(|e| OffloadError::Runtime(e.into()))?;
        self.prepared = Some(pre);
        Ok(())
    }

    fn call_raw(
        &self,
        export: &str,
        args: &[u8],
        policy: InstancePolicy,
    ) -> Result<Vec<u8>, OffloadError> {
        match policy {
            InstancePolicy::PerCall => {
                let mut fresh = self.instantiate()?;
                do_call(&mut fresh, export, args)
            }
            InstancePolicy::Shared => {
                let mut guard = self
                    .shared
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let shared = match guard.as_mut() {
                    Some(shared) => shared,
                    None => guard.insert(self.instantiate()?),
                };
                let result = do_call(shared, export, args);
                if result.is_err() {
                    *guard = None;
                }
                result
            }
        }
    }

    fn abi_version(&self) -> Result<u32, OffloadError> {
        let mut fresh = self.instantiate()?;
        let f: TypedFunc<(), u32> =
            typed_func(&mut fresh.store, &fresh.instance, ABI_VERSION_EXPORT)?;
        f.call(&mut fresh.store, ())
            .map_err(|e| OffloadError::GuestTrap(e.into()))
    }
}

fn do_call(
    shared: &mut SharedInstance,
    export: &str,
    args: &[u8],
) -> Result<Vec<u8>, OffloadError> {
    let len = u32::try_from(args.len())
        .map_err(|_| OffloadError::Runtime(anyhow::anyhow!("argument buffer exceeds 4 GiB")))?;

    let memory = shared.memory;
    let alloc = shared.alloc.clone();
    let free = shared.free.clone();
    let store = &mut shared.store;
    let entry: TypedFunc<(u32, u32), u64> = typed_func(store, &shared.instance, export)?;

    let ptr = alloc
        .call(&mut *store, len)
        .map_err(|e| OffloadError::GuestTrap(e.into()))?;
    memory
        .write(&mut *store, ptr as usize, args)
        .map_err(|e| OffloadError::Runtime(e.into()))?;

    let packed = entry
        .call(&mut *store, (ptr, len))
        .map_err(|e| OffloadError::GuestTrap(e.into()))?;

    let (ret_ptr, ret_len) = unpack_ret(packed);
    let ret_start = ret_ptr as usize;
    let ret_end = ret_start.checked_add(ret_len as usize);
    if ret_end.is_none_or(|end| end > memory.data_size(&*store)) {
        return Err(OffloadError::Runtime(anyhow::anyhow!(
            "guest returned buffer [{ret_ptr}, {}) outside its linear memory",
            u64::from(ret_ptr) + u64::from(ret_len)
        )));
    }
    let mut out = vec![0u8; ret_len as usize];
    memory
        .read(&*store, ret_start, &mut out)
        .map_err(|e| OffloadError::Runtime(e.into()))?;

    free.call(&mut *store, (ret_ptr, ret_len))
        .map_err(|error| OffloadError::GuestTrap(error.into()))?;
    free.call(&mut *store, (ptr, len))
        .map_err(|error| OffloadError::GuestTrap(error.into()))?;

    Ok(out)
}

fn typed_func<P, R>(
    store: &mut Store<WasiP1Ctx>,
    instance: &Instance,
    name: &str,
) -> Result<TypedFunc<P, R>, OffloadError>
where
    P: WasmParams,
    R: WasmResults,
{
    let func = instance
        .get_func(&mut *store, name)
        .ok_or_else(|| OffloadError::MissingExport(name.into()))?;
    func.typed(&*store)
        .map_err(|e| OffloadError::Runtime(e.into()))
}
