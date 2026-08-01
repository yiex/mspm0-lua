use std::env;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port_name = env::args().nth(1).ok_or("usage: serial_probe <serial-port>")?;
    let mut port = serialport::new(&port_name, 115_200)
        .data_bits(serialport::DataBits::Eight)
        .stop_bits(serialport::StopBits::One)
        .parity(serialport::Parity::None)
        .timeout(Duration::from_millis(50))
        .dtr_on_open(false)
        .open()?;
    let _ = port.write_data_terminal_ready(false);
    let _ = port.write_request_to_send(false);

    let mut buffer = [0u8; 1024];
    let drain_until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < drain_until {
        let _ = port.read(&mut buffer);
    }

    port.write_all(b"ls\r\n")?;
    port.flush()?;

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut response = Vec::new();
    while Instant::now() < deadline && !response.windows(6).any(|w| w == b"LS_END") {
        match port.read(&mut buffer) {
            Ok(n) => response.extend_from_slice(&buffer[..n]),
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
            Err(error) => return Err(error.into()),
        }
    }

    println!("SERIAL_PROBE_RX {}", response.len());
    print!("{}", String::from_utf8_lossy(&response));
    if response.windows(6).any(|w| w == b"LS_END") {
        println!("SERIAL_PROBE_OK");
        Ok(())
    } else {
        Err("missing LS_END".into())
    }
}
