#![cfg(target_arch = "wasm32")]

mod first {
    #[offload::offload]
    pub fn value() -> u32 {
        1
    }
}

mod second {
    #[offload::offload]
    pub fn value() -> u32 {
        2
    }
}

pub fn keep_functions_reachable() -> u32 {
    first::value() + second::value()
}
