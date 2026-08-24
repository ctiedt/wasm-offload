use std::fmt;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::MAX_FRAME_LEN;

const UART_POLL_TIMEOUT: Duration = Duration::from_secs(1);
const UART_WRITE_TIMEOUT: Duration = Duration::from_secs(300);

pub struct Connection {
    stream: Stream,
    write_timeout: Option<Duration>,
}

enum Stream {
    Tcp(TcpStream),
    Uart(Box<dyn serialport::SerialPort>),
}

impl Connection {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream: Stream::Tcp(stream),
            write_timeout: None,
        }
    }

    pub fn uart(device: impl Into<String>, baud_rate: u32) -> Result<Self, ConnError> {
        let device = device.into();
        let stream = serialport::new(device, baud_rate)
            .timeout(UART_POLL_TIMEOUT)
            .open()
            .map_err(map_serial)?;
        Ok(Self::from_uart(stream))
    }

    pub(crate) fn set_write_timeout(&mut self, timeout: Duration) -> Result<(), ConnError> {
        self.write_timeout = Some(timeout);
        self.stream.set_write_timeout(timeout)
    }

    pub fn send<T: Serialize>(&mut self, message: &T) -> Result<(), ConnError> {
        let payload = postcard::to_allocvec(message).map_err(ConnError::Encode)?;
        if payload.len() > MAX_FRAME_LEN as usize {
            return Err(ConnError::TooLarge(
                u32::try_from(payload.len()).unwrap_or(u32::MAX),
            ));
        }

        let length = payload.len() as u32;
        let mut frame = Vec::with_capacity(4 + payload.len());
        frame.extend_from_slice(&length.to_le_bytes());
        frame.extend_from_slice(&payload);
        if let Some(timeout) = self.write_timeout {
            self.stream.set_write_timeout(timeout)?;
        }
        self.stream.write_all(&frame).map_err(map_io)
    }

    pub fn recv<T: DeserializeOwned>(&mut self, timeout: Duration) -> Result<T, ConnError> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(ConnError::TimedOut)?;
        self.recv_inner(Some(deadline))
    }

    pub fn recv_blocking<T: DeserializeOwned>(&mut self) -> Result<T, ConnError> {
        self.recv_inner(None)
    }

    fn recv_inner<T: DeserializeOwned>(
        &mut self,
        deadline: Option<Instant>,
    ) -> Result<T, ConnError> {
        let mut prefix = [0u8; 4];
        self.read_exact(&mut prefix, deadline)?;
        let length = u32::from_le_bytes(prefix);
        if length > MAX_FRAME_LEN {
            return Err(ConnError::TooLarge(length));
        }

        let mut payload = vec![0u8; length as usize];
        self.read_exact(&mut payload, deadline)?;
        postcard::from_bytes(&payload).map_err(ConnError::Decode)
    }

    fn read_exact(
        &mut self,
        mut destination: &mut [u8],
        deadline: Option<Instant>,
    ) -> Result<(), ConnError> {
        while !destination.is_empty() {
            let timeout = match deadline {
                Some(deadline) => Some(remaining(deadline)?),
                None => None,
            };
            self.stream.set_read_timeout(timeout)?;

            match self.stream.read(destination) {
                Ok(0) => return Err(ConnError::Disconnected),
                Ok(read) => destination = &mut destination[read..],
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error)
                    if deadline.is_none()
                        && matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) =>
                {
                    continue;
                }
                Err(error) => return Err(map_io(error)),
            }
        }
        Ok(())
    }

    pub(crate) fn from_uart(stream: Box<dyn serialport::SerialPort>) -> Self {
        Self {
            stream: Stream::Uart(stream),
            write_timeout: Some(UART_WRITE_TIMEOUT),
        }
    }
}

impl Stream {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> Result<(), ConnError> {
        match self {
            Self::Tcp(stream) => stream.set_read_timeout(timeout).map_err(map_io),
            Self::Uart(stream) => stream
                .set_timeout(timeout.unwrap_or(UART_POLL_TIMEOUT))
                .map_err(map_serial),
        }
    }

    fn set_write_timeout(&mut self, timeout: Duration) -> Result<(), ConnError> {
        match self {
            Self::Tcp(stream) => stream.set_write_timeout(Some(timeout)).map_err(map_io),
            Self::Uart(stream) => stream.set_timeout(timeout).map_err(map_serial),
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(buffer),
            Self::Uart(stream) => stream.read(buffer),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(buffer),
            Self::Uart(stream) => stream.write(buffer),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            Self::Uart(stream) => stream.flush(),
        }
    }
}

fn remaining(deadline: Instant) -> Result<Duration, ConnError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or(ConnError::TimedOut)?;
    if remaining.is_zero() {
        Err(ConnError::TimedOut)
    } else {
        Ok(remaining)
    }
}

fn map_io(error: io::Error) -> ConnError {
    match error.kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => ConnError::TimedOut,
        io::ErrorKind::UnexpectedEof => ConnError::Disconnected,
        _ => ConnError::Io(error),
    }
}

fn map_serial(error: serialport::Error) -> ConnError {
    map_io(error.into())
}

#[derive(Debug)]
pub enum ConnError {
    Io(io::Error),
    TimedOut,
    Disconnected,
    TooLarge(u32),
    Encode(postcard::Error),
    Decode(postcard::Error),
}

impl fmt::Display for ConnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::TimedOut => formatter.write_str("operation timed out"),
            Self::Disconnected => formatter.write_str("peer disconnected"),
            Self::TooLarge(length) => write!(
                formatter,
                "frame length {length} exceeds the {MAX_FRAME_LEN}-byte limit"
            ),
            Self::Encode(error) => write!(formatter, "failed to encode frame: {error}"),
            Self::Decode(error) => write!(formatter, "failed to decode frame: {error}"),
        }
    }
}

impl std::error::Error for ConnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Encode(error) | Self::Decode(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    use crate::{HELLO_MAGIC, PROTOCOL_VERSION, Request};

    fn socket_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        (client, server)
    }

    #[test]
    fn framing_roundtrip() {
        let (client, server) = socket_pair();
        let mut sender = Connection::new(client);
        let mut receiver = Connection::new(server);
        sender
            .send(&Request::Hello {
                magic: HELLO_MAGIC,
                protocol_version: PROTOCOL_VERSION,
            })
            .unwrap();

        let request: Request = receiver.recv(Duration::from_secs(1)).unwrap();
        assert!(matches!(
            request,
            Request::Hello {
                magic: HELLO_MAGIC,
                protocol_version: PROTOCOL_VERSION,
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn uart_framing_roundtrip() {
        let (sender, receiver) = serialport::TTYPort::pair().unwrap();
        let mut sender = Connection::from_uart(Box::new(sender));
        let mut receiver = Connection::from_uart(Box::new(receiver));
        sender
            .send(&Request::Hello {
                magic: HELLO_MAGIC,
                protocol_version: PROTOCOL_VERSION,
            })
            .unwrap();

        let request: Request = receiver.recv(Duration::from_secs(1)).unwrap();
        assert!(matches!(
            request,
            Request::Hello {
                magic: HELLO_MAGIC,
                protocol_version: PROTOCOL_VERSION,
            }
        ));
    }

    #[test]
    fn oversized_frame_is_rejected_before_allocation() {
        let (mut client, server) = socket_pair();
        client
            .write_all(&(MAX_FRAME_LEN + 1).to_le_bytes())
            .unwrap();
        let mut receiver = Connection::new(server);
        let error = receiver
            .recv::<Request>(Duration::from_secs(1))
            .unwrap_err();
        assert!(matches!(error, ConnError::TooLarge(length) if length == MAX_FRAME_LEN + 1));
    }

    #[test]
    fn truncated_frame_is_disconnected() {
        let (mut client, server) = socket_pair();
        client.write_all(&4u32.to_le_bytes()).unwrap();
        client.write_all(&[0, 1]).unwrap();
        drop(client);

        let mut receiver = Connection::new(server);
        let error = receiver
            .recv::<Request>(Duration::from_secs(1))
            .unwrap_err();
        assert!(matches!(error, ConnError::Disconnected));
    }

    #[test]
    fn no_data_times_out() {
        let (_client, server) = socket_pair();
        let mut receiver = Connection::new(server);
        let error = receiver
            .recv::<Request>(Duration::from_millis(50))
            .unwrap_err();
        assert!(matches!(error, ConnError::TimedOut));
    }
}
