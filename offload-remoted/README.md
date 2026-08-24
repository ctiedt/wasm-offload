# `offload-remoted`

`offload-remoted` runs WebAssembly functions on a remote computer. The host
connects to the daemon through TCP or UART.

## Set up the target 

Install Rust 1.94.1 or a later version. Install the native build tools for your
operating system.

Start the daemon. Replace `10.0.0.2` with the wired IP address of the target
computer:

```sh
RUST_LOG=info cargo run --release -p offload-remoted --listen 10.0.0.2:8080
```

For UART, use the serial device exposed by the operating system and choose the
same baud rate on both computers:

```sh
RUST_LOG=info offload-remoted --uart /dev/ttyUSB0 --baud 115200
```

`--baud` defaults to `115200` when omitted. UART mode serves one long-lived
point-to-point session; restart both sides after a transport error.

See the root [README](../README.md) for configuration options.

## Connect the host

Set the same address in the host application:

```rust
offload::init_guest!(
    artifact = "mylib",
    target = offload::RemoteTarget::tcp("10.0.0.2:8080"),
)?;
```

To use UART instead, the application only changes the target passed through
`init_guest!`:

```rust
offload::init_guest!(
    artifact = "mylib",
    target = offload::RemoteTarget::uart("/dev/ttyUSB0", 115_200),
)?;
```

The guest module is transferred over the selected transport during
initialization. 
