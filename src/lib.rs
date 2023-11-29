extern crate serialport as serial;

use std::{
    env::Args,
    error::Error,
    fmt,
    io::{self, ErrorKind, Read, Write},
    process,
};

use gethostname::gethostname;
use serial::SerialPort;
use tcp_stream::{HandshakeError, TLSConfig, TcpStream};

pub fn run(program: &Program) -> Result<(), Box<dyn Error>> {
    let args = program.args.as_ref().unwrap();

    let username = gethostname().into_string().unwrap();

    let mut stream = create_tcp_stream(&args.host, args.port)?;

    let token = get_token(&mut stream, &username, &args.password, &args.host)?;

    let ports = find_ports()?;
    if ports.len() <= 0 {
        return Err(Box::new(io::Error::new(
            io::ErrorKind::NotFound,
            "No scanner found",
        )));
    }

    println!("Ports found:");
    for p in &ports {
        println!("{p}");
    }

    let port = serial::new(&ports[..][0], 9600)
        // .timeout(Duration::from_millis(10))
        .open()?;

    read_port(port, stream, &token, &args.postazione, &args.host)?;

    Ok(())
}

fn create_tcp_stream(domain: &str, port: u16) -> Result<TcpStream, Box<dyn Error>> {
    let mut stream = TcpStream::connect((domain, port))?;

    stream.set_nonblocking(true)?;

    while !stream.is_connected() {
        if stream.try_connect()? {
            break;
        }
    }

    let mut stream = stream.into_tls(domain, TLSConfig::default());
    while let Err(HandshakeError::WouldBlock(mid_handshake)) = stream {
        stream = mid_handshake.handshake();
    }

    let stream = stream?;

    Ok(stream)
}

fn get_token(
    stream: &mut TcpStream,
    username: &str,
    password: &str,
    host: &str,
) -> Result<String, Box<dyn Error>> {
    let body_request = format!(
        "{{\"username\":\"{}\",\"password\":\"{}\"}}",
        username, password
    );

    let request = format!(
        "\
    POST /api/v1/users/login HTTP/1.1\r\n\
    Host: {}\r\n\
    Content-Type: application/json; charset=utf-8\r\n\
    Content-Length: {}\r\n\r\n\
    {}",
        host,
        body_request.len(),
        body_request
    );

    while let Err(err) = stream.write_all(request.as_bytes()) {
        if err.kind() != io::ErrorKind::WouldBlock {
            return Err(Box::new(err));
        }
    }
    stream.flush()?;

    let mut response = vec![];
    while let Err(err) = stream.read_to_end(&mut response) {
        if err.kind() != io::ErrorKind::WouldBlock {
            return Err(Box::new(err));
        }
    }

    let response = std::str::from_utf8(&response)?;
    println!("{}", response);

    const HEADER_NAME: &str = "x-access-token: ";
    let token: String = response
        .lines()
        .filter_map(
            |line| match line.as_bytes().starts_with(HEADER_NAME.as_bytes()) {
                true => Some(&line[HEADER_NAME.len()..]),
                false => None,
            },
        )
        .collect();
    println!("{}", token);

    Ok(String::from(token))
}

fn find_ports() -> Result<Vec<String>, Box<dyn Error>> {
    let ports: Vec<String> = serial::available_ports()?
        .iter()
        .filter(|p| match p.port_type {
            serial::SerialPortType::UsbPort(_) => true,
            _ => false,
        })
        .map(|p| p.port_name.clone())
        .collect();

    Ok(ports)
}

fn read_port(
    mut port: Box<dyn SerialPort>,
    mut stream: TcpStream,
    token: &str,
    postazione: &str,
    host: &str,
) -> Result<(), Box<dyn Error>> {
    let mut buffer = [0u8; 32];

    loop {
        match port.read(&mut buffer) {
            Ok(n) => {
                let scanned = std::str::from_utf8(&buffer[..n])?;

                println!("{}", scanned);

                let body_req = format!(
                    "{{\"barcode\":\"{}\",\"postazioneId\":\"{}\"}}",
                    scanned, postazione
                );

                let request = format!(
                    "\
POST /api/v1/badges/archivio HTTP/1.1\r\n\
Host: {}\r\n\
x-access-token: {}\r\n\
Content-Type: application/json; charset=utf-8\r\n\
Content-Length: {}\r\n\r\n\
{}",
                    host,
                    token,
                    body_req.len(),
                    body_req
                );

                println!("{}", request);

                while let Err(err) = stream.write_all(request.as_bytes()) {
                    if err.kind() != io::ErrorKind::WouldBlock {
                        return Err(Box::new(err));
                    }
                }
                stream.flush()?;

                let mut response = vec![];
                while let Err(err) = stream.read_to_end(&mut response) {
                    if err.kind() != io::ErrorKind::WouldBlock {
                        return Err(Box::new(err));
                    }
                }

                let response = std::str::from_utf8(&response)?;
                println!("{}", response);
            }
            Err(e) => {
                if e.kind() != ErrorKind::TimedOut {
                    return Err(Box::new(e));
                }
            }
        }
    }
}

pub struct Program {
    name: String,
    args: Option<CmdArgs>,
}

impl Program {
    pub fn new(args: &mut Args) -> Program {
        let name = args.next().unwrap_or("barcode_scanner".to_string());

        Program { name, args: None }
    }

    pub fn get_args(&self) -> &Option<CmdArgs> {
        &self.args
    }

    pub fn set_args(&mut self, args: &mut Args) -> Result<(), Box<dyn Error>> {
        self.args = Some(CmdArgs::build(args)?);
        Ok(())
    }

    pub fn usage(&self) {
        eprintln!("usage: {} PASSWORD POSTAZIONE HOST PORT", self.name);
    }

    pub fn print_err(&self, err: Box<dyn Error>) {
        eprintln!("{}: error: {}", self.name, err);
    }

    pub fn exit(&self, status: i32) -> ! {
        process::exit(status);
    }

    pub fn fail(&self) -> ! {
        self.exit(1)
    }

    pub fn print_fail(&self, err: Box<dyn Error>) -> ! {
        self.print_err(err);
        self.fail();
    }
}

#[derive(Debug)]
pub struct CmdArgs {
    pub password: String,
    pub postazione: String,
    pub host: String,
    pub port: u16,
}

impl CmdArgs {
    fn build(args: &mut Args) -> Result<CmdArgs, Box<dyn Error>> {
        let password = args.next().ok_or(Box::new(CmdArgsBuildError))?;
        let postazione = args.next().ok_or(Box::new(CmdArgsBuildError))?;
        let host = args.next().unwrap_or("127.0.0.1".to_string());
        let port = args.next().unwrap_or("4316".to_string()).parse()?;

        Ok(CmdArgs {
            password,
            postazione,
            host,
            port,
        })
    }
}

#[derive(Debug)]
struct CmdArgsBuildError;
impl fmt::Display for CmdArgsBuildError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CmdArgsBuildError")
    }
}
impl Error for CmdArgsBuildError {}
