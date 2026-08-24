use std::error::Error;
use std::net::TcpListener;

use clap::Parser;
use offload_host::WasiConfig;
use offload_remote::Connection;
use offload_remoted::{SessionOpts, serve};

const DEFAULT_LISTEN: &str = "0.0.0.0:8080";
const DEFAULT_UART_BAUD: u32 = 115_200;

#[derive(Debug, Parser)]
#[command(about = "Serve wasm-offload requests over TCP or UART")]
struct Cli {
    #[arg(long, value_name = "ADDRESS", conflicts_with = "uart")]
    listen: Option<String>,

    #[arg(long, value_name = "DEVICE", conflicts_with = "listen")]
    uart: Option<String>,

    #[arg(long, value_name = "BAUD", requires = "uart")]
    baud: Option<u32>,

    #[arg(long)]
    wasi_stdout: bool,

    #[arg(long)]
    wasi_stderr: bool,

    #[arg(long)]
    pooling: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    let cli = Cli::parse();
    let opts = SessionOpts {
        wasi: WasiConfig::new()
            .inherit_stdout(cli.wasi_stdout)
            .inherit_stderr(cli.wasi_stderr),
        pooling: cli.pooling,
    };

    match cli.uart.as_deref() {
        Some(device) => serve_uart(device, cli.baud.unwrap_or(DEFAULT_UART_BAUD), &opts),
        None => serve_tcp(cli.listen.as_deref().unwrap_or(DEFAULT_LISTEN), &opts),
    }
}

fn serve_tcp(listen: &str, opts: &SessionOpts) -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind(listen)?;
    let local_addr = listener.local_addr()?;
    log::info!("offload-remoted listening on {local_addr}");

    for accepted in listener.incoming() {
        match accepted {
            Ok(stream) => {
                let peer = stream
                    .peer_addr()
                    .map(|addr| addr.to_string())
                    .unwrap_or_else(|_| "<unknown>".into());
                log::info!("session started for {peer}");
                let mut conn = Connection::new(stream);
                match serve(&mut conn, opts) {
                    Ok(()) => log::info!("session ended for {peer}"),
                    Err(error) => log::warn!("session ended for {peer}: {error}"),
                }
            }
            Err(error) => log::warn!("failed to accept connection: {error}"),
        }
    }
    Ok(())
}

fn serve_uart(device: &str, baud: u32, opts: &SessionOpts) -> Result<(), Box<dyn Error>> {
    log::info!("opening UART device {device} at {baud} baud");
    let mut conn = Connection::uart(device, baud)?;
    log::info!("UART session started on {device}");
    serve(&mut conn, opts)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uart_cli_accepts_a_device_and_baud_rate() {
        let cli = Cli::try_parse_from([
            "offload-remoted",
            "--uart",
            "/dev/ttyUSB0",
            "--baud",
            "921600",
        ])
        .unwrap();
        assert_eq!(cli.uart.as_deref(), Some("/dev/ttyUSB0"));
        assert_eq!(cli.baud, Some(921_600));
    }

    #[test]
    fn tcp_and_uart_options_are_mutually_exclusive() {
        let error = Cli::try_parse_from([
            "offload-remoted",
            "--listen",
            "127.0.0.1:8080",
            "--uart",
            "/dev/ttyUSB0",
        ])
        .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
}
