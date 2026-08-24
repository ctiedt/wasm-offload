use offload_core::OffloadTarget;
use offload_host::{WasiConfig, WasmtimeTarget};
use offload_remote::{
    ConnError, Connection, HELLO_ACK_MAGIC, HELLO_MAGIC, PROTOCOL_VERSION, Request, Response,
    WireError, to_wire,
};

pub struct SessionOpts {
    pub wasi: WasiConfig,
    pub pooling: bool,
}

pub fn serve(conn: &mut Connection, opts: &SessionOpts) -> Result<(), ConnError> {
    let hello: Request = receive(conn)?;
    match hello {
        Request::Hello {
            magic: HELLO_MAGIC,
            protocol_version: PROTOCOL_VERSION,
        } => {}
        Request::Hello {
            magic,
            protocol_version,
        } => {
            conn.send(&Response::Err(WireError::Protocol(format!(
                "handshake mismatch: expected magic {HELLO_MAGIC:#010x} and protocol v{PROTOCOL_VERSION}, got magic {magic:#010x} and protocol v{protocol_version}"
            ))))?;
            return Ok(());
        }
        other => {
            conn.send(&Response::Err(WireError::Protocol(format!(
                "first request must be Hello, got {other:?}"
            ))))?;
            return Ok(());
        }
    }
    conn.send(&Response::Hello {
        magic: HELLO_ACK_MAGIC,
        protocol_version: PROTOCOL_VERSION,
    })?;

    let mut target: Option<WasmtimeTarget> = None;
    loop {
        let request: Request = receive(conn)?;
        match request {
            Request::Hello {
                magic: HELLO_MAGIC,
                protocol_version: PROTOCOL_VERSION,
            } => {
                target = None;
                log::info!("restarting session after a new handshake");
                conn.send(&Response::Hello {
                    magic: HELLO_ACK_MAGIC,
                    protocol_version: PROTOCOL_VERSION,
                })?;
            }
            Request::Hello {
                magic,
                protocol_version,
            } => {
                conn.send(&Response::Err(WireError::Protocol(format!(
                    "handshake mismatch: expected magic {HELLO_MAGIC:#010x} and protocol v{PROTOCOL_VERSION}, got magic {magic:#010x} and protocol v{protocol_version}"
                ))))?;
            }
            Request::Prepare { module, policy } => {
                target = None;
                log::info!("preparing {}-byte guest module", module.len());
                let prepared = if opts.pooling {
                    WasmtimeTarget::with_pooling(opts.wasi)
                } else {
                    Ok(WasmtimeTarget::new(opts.wasi))
                }
                .and_then(|mut new_target| {
                    new_target.prepare(&module, policy)?;
                    Ok(new_target)
                });

                match prepared {
                    Ok(new_target) => {
                        target = Some(new_target);
                        log::info!("guest module prepared");
                        conn.send(&Response::Prepared)?;
                    }
                    Err(error) => {
                        log::warn!("guest preparation failed: {error:#}");
                        conn.send(&Response::Err(to_wire(&error)))?;
                    }
                }
            }
            Request::AbiVersion => match target.as_ref() {
                Some(target) => match target.abi_version() {
                    Ok(version) => conn.send(&Response::AbiVersion(version))?,
                    Err(error) => conn.send(&Response::Err(to_wire(&error)))?,
                },
                None => conn.send(&Response::Err(WireError::Protocol(
                    "AbiVersion received before Prepare".into(),
                )))?,
            },
            Request::Call { export, args } => match target.as_ref() {
                Some(target) => match target.call_raw(&export, &args) {
                    Ok(result) => conn.send(&Response::Called(result))?,
                    Err(error) => conn.send(&Response::Err(to_wire(&error)))?,
                },
                None => conn.send(&Response::Err(WireError::Protocol(
                    "Call received before Prepare".into(),
                )))?,
            },
        }
    }
}

fn receive(conn: &mut Connection) -> Result<Request, ConnError> {
    conn.recv_blocking().inspect_err(|error| {
        log::warn!("closing session after a receive error: {error}");
    })
}
