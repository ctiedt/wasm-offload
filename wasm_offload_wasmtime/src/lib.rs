use thiserror::Error;
use wasm_offload::{OffloadTarget, Val};
use wasmtime::{component::Component, component::Linker, AsContextMut, Engine, Store};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};

#[macro_export]
macro_rules! init_offload {
    () => {
        static OFFLOADER: std::sync::LazyLock<
            std::sync::Arc<std::sync::Mutex<wasm_offload_wasmtime::WasmtimeOffload>>,
        > = std::sync::LazyLock::new(|| {
            std::sync::Arc::new(std::sync::Mutex::new(
                wasm_offload_wasmtime::WasmtimeOffload::new().unwrap(),
            ))
        });
    };
}

trait WtValExt {
    fn to_offload(&self) -> Val;
}

impl WtValExt for wasmtime::component::Val {
    fn to_offload(&self) -> Val {
        match self {
            wasmtime::component::Val::Bool(v) => Val::Bool(*v),
            wasmtime::component::Val::S8(v) => Val::S8(*v),
            wasmtime::component::Val::U8(v) => Val::U8(*v),
            wasmtime::component::Val::S16(v) => Val::S16(*v),
            wasmtime::component::Val::U16(v) => Val::U16(*v),
            wasmtime::component::Val::S32(v) => Val::S32(*v),
            wasmtime::component::Val::U32(v) => Val::U32(*v),
            wasmtime::component::Val::S64(v) => Val::S64(*v),
            wasmtime::component::Val::U64(v) => Val::U64(*v),
            wasmtime::component::Val::Float32(v) => Val::Float32(*v),
            wasmtime::component::Val::Float64(v) => Val::Float64(*v),
            wasmtime::component::Val::Char(v) => Val::Char(*v),
            wasmtime::component::Val::String(v) => Val::String(v.clone()),
            wasmtime::component::Val::List(v) => {
                Val::List(v.into_iter().map(WtValExt::to_offload).collect())
            }
            wasmtime::component::Val::Record(v) => Val::Record(
                v.into_iter()
                    .map(|(a, b)| (a.clone(), b.to_offload()))
                    .collect(),
            ),
            wasmtime::component::Val::Tuple(v) => {
                Val::Tuple(v.into_iter().map(WtValExt::to_offload).collect())
            }
            wasmtime::component::Val::Variant(v, w) => {
                Val::Variant(v.clone(), w.as_ref().map(|w| Box::new(w.to_offload())))
            }
            wasmtime::component::Val::Enum(v) => Val::Enum(v.clone()),
            wasmtime::component::Val::Option(v) => {
                Val::Option(v.as_ref().map(|v| Box::new(v.to_offload())))
            }
            wasmtime::component::Val::Result(v) => Val::Result(match v {
                Ok(o) => Ok(o.as_ref().map(|v| Box::new(v.to_offload()))),
                Err(e) => Err(e.as_ref().map(|v| Box::new(v.to_offload()))),
            }),
            wasmtime::component::Val::Flags(v) => Val::Flags(v.clone()),
            wasmtime::component::Val::Resource(_) => unimplemented!(),
        }
    }
}

trait ValExt {
    fn to_wasmtime(&self) -> wasmtime::component::Val;
}

impl ValExt for Val {
    fn to_wasmtime(&self) -> wasmtime::component::Val {
        match self {
            Val::Bool(v) => wasmtime::component::Val::Bool(*v),
            Val::S8(v) => wasmtime::component::Val::S8(*v),
            Val::U8(v) => wasmtime::component::Val::U8(*v),
            Val::S16(v) => wasmtime::component::Val::S16(*v),
            Val::U16(v) => wasmtime::component::Val::U16(*v),
            Val::S32(v) => wasmtime::component::Val::S32(*v),
            Val::U32(v) => wasmtime::component::Val::U32(*v),
            Val::S64(v) => wasmtime::component::Val::S64(*v),
            Val::U64(v) => wasmtime::component::Val::U64(*v),
            Val::Float32(v) => wasmtime::component::Val::Float32(*v),
            Val::Float64(v) => wasmtime::component::Val::Float64(*v),
            Val::Char(v) => wasmtime::component::Val::Char(*v),
            Val::String(v) => wasmtime::component::Val::String(v.clone()),
            Val::List(v) => {
                wasmtime::component::Val::List(v.into_iter().map(ValExt::to_wasmtime).collect())
            }
            Val::Record(v) => wasmtime::component::Val::Record(
                v.into_iter()
                    .map(|(a, b)| (a.clone(), b.to_wasmtime()))
                    .collect(),
            ),
            Val::Tuple(v) => {
                wasmtime::component::Val::Tuple(v.into_iter().map(ValExt::to_wasmtime).collect())
            }
            Val::Variant(v, w) => wasmtime::component::Val::Variant(
                v.clone(),
                w.as_ref().map(|w| Box::new(w.to_wasmtime())),
            ),
            Val::Enum(v) => wasmtime::component::Val::Enum(v.clone()),
            Val::Option(v) => {
                wasmtime::component::Val::Option(v.as_ref().map(|v| Box::new(v.to_wasmtime())))
            }
            Val::Result(v) => wasmtime::component::Val::Result(match v {
                Ok(o) => Ok(o.as_ref().map(|v| Box::new(v.to_wasmtime()))),
                Err(e) => Err(e.as_ref().map(|v| Box::new(v.to_wasmtime()))),
            }),
            Val::Flags(v) => wasmtime::component::Val::Flags(v.clone()),
        }
    }
}

#[derive(Error, Debug)]
pub enum WasmtimeOffloadError {
    #[error("wasmtime error")]
    Wasmtime(#[from] wasmtime::Error),
}

pub struct OffloaderState {
    ctx: WasiCtx,
    table: ResourceTable,
}

impl WasiView for OffloaderState {
    fn table(&mut self) -> &mut wasmtime_wasi::ResourceTable {
        &mut self.table
    }

    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.ctx
    }
}

pub struct WasmtimeOffload {
    engine: Engine,
    store: Store<OffloaderState>,
    linker: Linker<OffloaderState>,
}

impl WasmtimeOffload {
    pub fn new() -> Result<Self, WasmtimeOffloadError> {
        let engine = Engine::default();
        let mut linker = Linker::<OffloaderState>::new(&engine);
        wasmtime_wasi::add_to_linker_sync(&mut linker)?;
        // preview1::add_to_linker_sync(&mut linker, |t| t);

        let mut builder = WasiCtxBuilder::new();

        let store = Store::new(
            &engine,
            OffloaderState {
                ctx: builder.build(),
                table: ResourceTable::new(),
            },
        );

        Ok(Self {
            engine,
            store,
            linker,
        })
    }
}

impl OffloadTarget for WasmtimeOffload {
    type Error = WasmtimeOffloadError;

    fn initialize(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn call_function(
        &mut self,
        module: &[u8],
        name: &str,
        args: &[wasm_offload::Val],
        returns: bool,
    ) -> Result<Option<Val>, Self::Error> {
        // let module = Module::new(&self.engine, module)?;
        // let mut ctx = self.store.as_context_mut();
        // let instance = self.linker.instantiate(&mut ctx, &module)?;
        // let func = instance.get_func(&mut ctx, name).unwrap();

        let component = Component::new(&self.engine, module)?;
        let mut ctx = self.store.as_context_mut();
        let instance = self.linker.instantiate(&mut ctx, &component)?;
        let func = instance.get_func(&mut ctx, name).unwrap();

        // let mut output = Vec::with_capacity(1);
        let mut output = if returns {
            vec![wasmtime::component::Val::U32(0)]
        } else {
            vec![]
        };
        let args: Vec<_> = args.iter().map(ValExt::to_wasmtime).collect();
        func.call(&mut ctx, &args, &mut output)?;
        Ok(output.get(0).map(WtValExt::to_offload))
    }
}
