# WASM Offload

This project provides a set of crates to offload Rust functions to a WebAssembly
runtime. Just annotate any functions you want to offload with `#[offload]` and
define your `OffloadTarget` and your code will seamlessly run in WebAssembly.
An implementation for [wasmtime](https://github.com/bytecodealliance/wasmtime) is already provided.

## Design
The crates works by compiling your code twice, once for your regular target and once for WASM, where each pass expands the offloaded functions differently using macros and implementing a small specialized rpc framework to seamlessly call them (transparent to the developer). This requires either nightly (bindeps) and the offloaded functions to be in a library or using the provided build script.
The default WASM target is `wasm32-wasip1`.

## Recommended setup
The recommended setup is to use a workspace and put the offloaded functions into a library (requires nightly):

Library `Cargo.toml`:
```toml
[lib]
crate-type = ["lib", "cdylib"]

[dependencies]
offload = "0.1"
serde = { version = "1", features = ["derive"] }
```

Library `Cargo.toml`:
```toml
[dependencies]
offload = "0.1"
mylib = {
    path = "../mylib",
    artifact = "cdylib",
    target = "wasm32-wasip1",
    lib = true,
}

```

Workspace `.cargo/config.toml`:
```toml
[unstable]
bindeps = true
```

Then your library code can look like this:
```Rust
use offload::offload;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[offload]
pub fn distance(a: Point, b: Point) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    (dx * dx + dy * dy).sqrt()
}
```

And your application code like this:
```Rust
fn main() -> Result<(), offload::OffloadError> {
    offload::init_guest!(artifact = "mylib")?;

    let distance = mylib::distance(
        mylib::Point { x: 0.0, y: 0.0 },
        mylib::Point { x: 3.0, y: 4.0 },
    );

    println!("{distance}");
    Ok(())
}
```


`"my-lib"` is the name of the library containing the offloaded functions.

## Alternative with build script

This setup does **not** require nightly and works for a single crate (no workspace needed).

`Cargo.toml`:
```toml
[lib]
crate-type = ["cdylib"]
path = "src/main.rs" // or wherever the file containing your functions is

[build-dependencies]
offload-build = "0.1"
```

Create a `build.rs` file and add:
```Rust
fn main() {
    offload_build::GuestBuilder::new("name")
        .build()
        .unwrap();
}
```

`"name"` is the name of the package containing the offloaded functions.

The `GuestBuilder` contains several options to configure the guest:
```Rust
GuestBuilder::new("mylib")
    .target("wasm32-wasip1")
    .profile("release")
    .manifest_path("path/to/Cargo.toml")
    .feature("feature-a")
    .feature("feature-b")
    .no_default_features(true)
    .build()?;
```
**Note:** This belongs in `build.rs`.

## Initialization
The `init_guest!` macro is provided for easy and ergonomic Initialization:
```Rust
offload::init_guest!(
    artifact = "mylib",
    instance_policy = offload::InstancePolicy::Shared,
    wasi = offload::WasiConfig::new().inherit_output(),
    pooling_allocator = true,
)?;
```
**Note:** When using the nightly bindeps approach, the artifact option must be manually set. It can be omitted when using the build script.

It also provides several configuration options:
### Instance policies
- `InstancePolicy::PerCall` (default) creates a new instance for each call, so global states resets between calls. This prevents faults from propagating between calls.
- `InstancePolicy::Shared` preserves a single guest instance for all calls, making it possible to retain global state in the guest between calls. It uses a mutex and can therefore not be used concurrently.

### WASI configuration
It specifies the configuration used for WASI:
```Rust
let wasi = offload::WasiConfig::new()
    .inherit_stdin(true)
    .inherit_stdout(true)
    .inherit_stderr(true);

// inherit_output enables stdout and stderr together
offload::WasiConfig::new()
    .inherit_output()
```

### Wasmtime settings
Use options like Wasmtime's pooling allocator (see example above)

### Target
Specify a custom target to be used:
```Rust
offload::init_guest!(
    artifact = "mylib",
    target = CustomTarget::new(),
)?;
```

It has to implement the trait `offload::OffloadTarget`.

The provided remote target can connect through TCP or a UART device:
```Rust
offload::init_guest!(
    artifact = "mylib",
    target = offload::RemoteTarget::tcp("10.0.0.2:8080"),
)?;

offload::init_guest!(
    artifact = "mylib",
    target = offload::RemoteTarget::uart("/dev/ttyUSB0", 115_200),
)?;
```

The remote computer must run `offload-remoted` with the matching TCP address or
UART device. See [`offload-remoted`](offload-remoted/README.md) for setup.

If the remote device may be unavailable, enable `fallback_to_local` to run the guest
locally with wasmtime instead (using the configured WASI and pooling allocator settings):
```Rust
offload::init_guest!(
    artifact = "mylib",
    target = offload::RemoteTarget::tcp("10.0.0.2:8080"),
    fallback_to_local = true,
)?;

if offload::global()?.is_local_fallback() {
    eprintln!("remote target unreachable, running locally");
}
```
The fallback only happens during initialization and only for transport errors (e.g. the
device or address cannot be reached or the handshake times out). Errors reported by the
remote itself, such as a rejected module, are still returned. If the connection is lost
after initialization, calls return `OffloadError::Transport` and do not fall back.

## Settings

Per default, the original signature is preserved:
```Rust
#[offload]
fn foo(...) -> T
```

Adding the `try`-attribute to the macro wraps the return type in a result (things like wasm runtime panics are returned as error in your host code then):
```Rust
#[offload(try)]
fn foo(...) -> Result<T, offload::OffloadError>
```

It is also possible to give the exported function another name:
```Rust
#[offload(export = "custom-name")]
```

## References

- Immutable references, e.g. `&T`, are copied and then serialized. Changes through internal mutability, such as `Cell`, `RefCell` or `Mutex`, are NOT sent back and do therefore NOT mutate the original object.
- Mutable references are copied, serialized and then sent back in their changed version.
- `&str` and `&mut str` are converted to `String` for serialization. 
- Slices, e.g. `&[T]` and `&mut [T]` are converted to `Vec<T>` for serialization.

Mutable arguments are sent back in their updated state after the call completes to update the state in the host accordingly. If the call itself fails, those arguments are NOT sent back and the state of them in the host remains the same as before the call. 

Each reference argument is serialized independently, so two references to the same object become two separate guest objects.

Returning references is not supported. Although technically possible, the host would have no owner for the referenced value, making lifetimes awkward. You can return owned types like `String` or `Vec<T>` instead.

## Limitations
The following common features cannot be used with the `#[offload]` macro:
- References in return types or nested inside boundary types
- Explicit `'static` argument references
- `impl Trait` (use a concrete owned type)
- `const` functions
- `async` functions
- Generic functions
- Functions with a `where`
- `extern` functions
- Variadic functions
- Methods
- Be aware of pointer-sized integers (e.g. `usize`/`isize`), as they are 32-Bit on the WASM side but (probably) 64-Bit on the host side.

Owned boundary values and reference targets must implement `serde::Serialize` and
`serde::de::DeserializeOwned`.
