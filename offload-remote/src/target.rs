use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::Context;
use offload_core::{InstancePolicy, OffloadError, OffloadTarget};

use crate::{
    ConnError, Connection, HELLO_ACK_MAGIC, HELLO_MAGIC, PROTOCOL_VERSION, Request, Response,
};

const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_RESPONSE_TIMEOUT: Duration = Duration::from_secs(300);

pub struct RemoteTarget {
    endpoint: Endpoint,
    handshake_timeout: Duration,
    response_timeout: Duration,
    conn: Mutex<Option<Connection>>,
}

enum Endpoint {
    Tcp(String),
    Uart { device: String, baud_rate: u32 },
}

impl RemoteTarget {
    pub fn tcp(addr: impl Into<String>) -> Self {
        Self {
            endpoint: Endpoint::Tcp(addr.into()),
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
            response_timeout: DEFAULT_RESPONSE_TIMEOUT,
            conn: Mutex::new(None),
        }
    }

    pub fn uart(device: impl Into<String>, baud_rate: u32) -> Self {
        Self {
            endpoint: Endpoint::Uart {
                device: device.into(),
                baud_rate,
            },
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
            response_timeout: DEFAULT_RESPONSE_TIMEOUT,
            conn: Mutex::new(None),
        }
    }

    pub fn handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
        self
    }

    pub fn response_timeout(mut self, timeout: Duration) -> Self {
        self.response_timeout = timeout;
        self
    }

    fn request(
        &self,
        request: Request,
        expected: ExpectedResponse,
    ) -> Result<Response, OffloadError> {
        let mut guard = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(conn) = guard.as_mut() else {
            return Err(transport(
                "connection lost or remote target was not prepared",
            ));
        };

        if let Err(error) = conn.send(&request) {
            guard.take();
            return Err(conn_transport("send request", error));
        }
        let response = match conn.recv(self.response_timeout) {
            Ok(response) => response,
            Err(error) => {
                guard.take();
                return Err(conn_transport("receive response", error));
            }
        };

        if matches!(response, Response::Err(_)) || expected.matches(&response) {
            return Ok(response);
        }

        guard.take();
        Err(transport(format!(
            "unexpected response to {}: {response:?}",
            expected.request_name()
        )))
    }

    fn connect(&self, deadline: Instant) -> Result<Connection, OffloadError> {
        let mut conn = match &self.endpoint {
            Endpoint::Tcp(addr) => {
                let socket_addr = addr
                    .to_socket_addrs()
                    .with_context(|| format!("failed to resolve remote address `{addr}`"))
                    .map_err(OffloadError::Transport)?
                    .next()
                    .ok_or_else(|| {
                        transport(format!("remote address `{addr}` resolved to no endpoints"))
                    })?;
                let connect_timeout = remaining(deadline, "TCP connect")?;
                let stream = TcpStream::connect_timeout(&socket_addr, connect_timeout)
                    .with_context(|| format!("failed to connect to {socket_addr}"))
                    .map_err(OffloadError::Transport)?;
                stream
                    .set_nodelay(true)
                    .context("failed to enable TCP_NODELAY")
                    .map_err(OffloadError::Transport)?;
                Connection::new(stream)
            }
            Endpoint::Uart { device, baud_rate } => Connection::uart(device.clone(), *baud_rate)
                .map_err(|error| {
                    conn_transport(
                        &format!("open UART device `{device}` at {baud_rate} baud"),
                        error,
                    )
                })?,
        };
        conn.set_write_timeout(self.response_timeout)
            .map_err(|error| conn_transport("configure write timeout", error))?;
        Ok(conn)
    }
}

impl OffloadTarget for RemoteTarget {
    fn prepare(&mut self, module: &[u8], policy: InstancePolicy) -> Result<(), OffloadError> {
        self.conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();

        let deadline = Instant::now()
            .checked_add(self.handshake_timeout)
            .ok_or_else(|| transport("handshake timeout is too large"))?;
        let endpoint = self.endpoint.description();
        let mut conn = self.connect(deadline)?;

        conn.send(&Request::Hello {
            magic: HELLO_MAGIC,
            protocol_version: PROTOCOL_VERSION,
        })
        .map_err(|error| conn_transport("send handshake", error))?;
        let response: Response = conn
            .recv(remaining(deadline, "handshake response")?)
            .map_err(|error| conn_transport("receive handshake", error))?;
        match response {
            Response::Hello {
                magic: HELLO_ACK_MAGIC,
                protocol_version: PROTOCOL_VERSION,
            } => {}
            Response::Err(error) => return Err(error.into()),
            other => {
                return Err(transport(format!(
                    "invalid handshake response from {endpoint}: {other:?}"
                )));
            }
        }

        conn.send(&Request::Prepare {
            module: module.to_vec(),
            policy,
        })
        .map_err(|error| conn_transport("send module", error))?;
        let response: Response = conn
            .recv(self.response_timeout)
            .map_err(|error| conn_transport("receive prepare response", error))?;
        match response {
            Response::Prepared => {
                *self
                    .conn
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(conn);
                Ok(())
            }
            Response::Err(error) => {
                *self
                    .conn
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(conn);
                Err(error.into())
            }
            other => Err(transport(format!(
                "unexpected response to Prepare: {other:?}"
            ))),
        }
    }

    fn call_raw(&self, export: &str, args: &[u8]) -> Result<Vec<u8>, OffloadError> {
        match self.request(
            Request::Call {
                export: export.to_owned(),
                args: args.to_vec(),
            },
            ExpectedResponse::Called,
        )? {
            Response::Called(bytes) => Ok(bytes),
            Response::Err(error) => Err(error.into()),
            _ => unreachable!("request validates the response variant"),
        }
    }

    fn abi_version(&self) -> Result<u32, OffloadError> {
        match self.request(Request::AbiVersion, ExpectedResponse::AbiVersion)? {
            Response::AbiVersion(version) => Ok(version),
            Response::Err(error) => Err(error.into()),
            _ => unreachable!("request validates the response variant"),
        }
    }
}

impl Endpoint {
    fn description(&self) -> String {
        match self {
            Self::Tcp(addr) => format!("TCP endpoint {addr}"),
            Self::Uart { device, baud_rate } => {
                format!("UART device `{device}` at {baud_rate} baud")
            }
        }
    }
}

#[derive(Clone, Copy)]
enum ExpectedResponse {
    AbiVersion,
    Called,
}

impl ExpectedResponse {
    fn matches(self, response: &Response) -> bool {
        matches!(
            (self, response),
            (Self::AbiVersion, Response::AbiVersion(_)) | (Self::Called, Response::Called(_))
        )
    }

    fn request_name(self) -> &'static str {
        match self {
            Self::AbiVersion => "AbiVersion",
            Self::Called => "Call",
        }
    }
}

fn remaining(deadline: Instant, operation: &str) -> Result<Duration, OffloadError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| transport(format!("{operation} timed out")))?;
    Ok(remaining)
}

fn conn_transport(context: &str, error: ConnError) -> OffloadError {
    OffloadError::Transport(anyhow::Error::new(error).context(context.to_owned()))
}

fn transport(message: impl Into<String>) -> OffloadError {
    OffloadError::Transport(anyhow::anyhow!(message.into()))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use offload_core::ABI_VERSION;
    use serialport::{SerialPort, TTYPort};

    #[cfg(target_os = "macos")]
    const PTY_BAUD: u32 = 0;
    #[cfg(not(target_os = "macos"))]
    const PTY_BAUD: u32 = 115_200;

    #[test]
    fn uart_target_runs_the_remote_protocol() {
        let (server_port, client_port) = TTYPort::pair().unwrap();
        let client_device = client_port.name().unwrap();
        drop(client_port);
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();

        let server = std::thread::spawn(move || {
            let mut conn = Connection::from_uart(Box::new(server_port));

            let hello: Request = loop {
                match conn.recv_blocking() {
                    Ok(hello) => break hello,
                    Err(ConnError::Io(error))
                        if error.kind() == std::io::ErrorKind::BrokenPipe
                            || error.raw_os_error() == Some(5) =>
                    {
                        std::thread::yield_now();
                    }
                    Err(error) => panic!("receive handshake: {error}"),
                }
            };
            assert!(matches!(
                hello,
                Request::Hello {
                    magic: HELLO_MAGIC,
                    protocol_version: PROTOCOL_VERSION,
                }
            ));
            conn.send(&Response::Hello {
                magic: HELLO_ACK_MAGIC,
                protocol_version: PROTOCOL_VERSION,
            })
            .unwrap();

            let prepare: Request = conn.recv_blocking().unwrap();
            assert!(matches!(
                prepare,
                Request::Prepare {
                    ref module,
                    policy: InstancePolicy::Shared,
                } if module == b"test module"
            ));
            conn.send(&Response::Prepared).unwrap();

            let abi_version: Request = conn.recv_blocking().unwrap();
            assert!(matches!(abi_version, Request::AbiVersion));
            conn.send(&Response::AbiVersion(ABI_VERSION)).unwrap();

            let call: Request = conn.recv_blocking().unwrap();
            assert!(matches!(
                call,
                Request::Call { ref export, ref args }
                    if export == "__offload_test" && args == b"arguments"
            ));
            conn.send(&Response::Called(b"result".to_vec())).unwrap();
            finished_rx.recv().unwrap();
        });

        let mut target = RemoteTarget::uart(client_device, PTY_BAUD);
        target
            .prepare(b"test module", InstancePolicy::Shared)
            .unwrap();
        assert_eq!(target.abi_version().unwrap(), ABI_VERSION);
        assert_eq!(
            target.call_raw("__offload_test", b"arguments").unwrap(),
            b"result"
        );
        finished_tx.send(()).unwrap();
        server.join().unwrap();
    }
}
