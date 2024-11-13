use wasmtime::{
    component::{Component, Linker},
    AsContextMut, Engine, Store,
};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let module = include_bytes!("../../output2.wasm");
        let engine = Engine::default();
        let mut linker = Linker::<OffloaderState>::new(&engine);
        wasmtime_wasi::add_to_linker_sync(&mut linker)?;

        let mut builder = WasiCtxBuilder::new();

        let mut store = Store::new(
            &engine,
            OffloaderState {
                ctx: builder.build(),
                table: ResourceTable::new(),
            },
        );

        let component = Component::new(&engine, module)?;
        let mut ctx = store.as_context_mut();
        let instance = linker.instantiate(&mut ctx, &component)?;
        instance.get_module(&mut ctx, "0").unwrap();

        Ok(())
}
