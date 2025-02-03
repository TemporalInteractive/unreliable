use anyhow::Result;
use core::{
    clone::Clone,
    convert::From,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    str::FromStr,
    time::Duration,
};
use crossbeam_channel::{Receiver, Sender};
use std::{
    net::{TcpListener, TcpStream, UdpSocket},
    sync::{Arc, Mutex},
    thread,
};

pub enum PacketType {
    Unreliable,
    Barrier,
}

pub struct Packet {
    addr: SocketAddr,
    payload: Box<[u8]>,
    ty: PacketType,
}

impl Packet {
    fn unreliable(addr: SocketAddr, payload: Vec<u8>) -> Self {
        Self {
            addr,
            payload: payload.into_boxed_slice(),
            ty: PacketType::Unreliable,
        }
    }

    fn barrier(addr: SocketAddr, payload: Vec<u8>) -> Self {
        Self {
            addr,
            payload: payload.into_boxed_slice(),
            ty: PacketType::Barrier,
        }
    }
}

pub struct PacketSender {
    packet_sender: Sender<Packet>,
    timeframe: u32,
}

impl PacketSender {
    fn new(packet_sender: Sender<Packet>) -> Self {
        Self {
            packet_sender,
            timeframe: 0,
        }
    }

    pub fn send_unreliable(&mut self, addr: SocketAddr, mut payload: Vec<u8>) -> Result<()> {
        payload.append(&mut bytemuck::bytes_of(&self.timeframe).to_vec());
        let packet = Packet::unreliable(addr, payload);

        self.packet_sender.try_send(packet)?;
        Ok(())
    }

    pub fn send_barrier(&mut self, addr: SocketAddr) -> Result<()> {
        self.timeframe += 1;

        let payload = bytemuck::bytes_of(&self.timeframe).to_vec();
        let packet = Packet::barrier(addr, payload);

        self.packet_sender.try_send(packet)?;
        Ok(())
    }
}

pub enum SocketEvent {
    Packet(Packet),
    Connect(SocketAddr),
    Disconnect(SocketAddr),
}

struct Connection {
    udp_socket: UdpSocket,
    tcp_stream: TcpStream,
}

impl Connection {
    fn new(tcp_stream: TcpStream) -> Self {
        let udp_socket = UdpSocket::bind(tcp_stream.peer_addr().unwrap()).unwrap();
        Self {
            udp_socket,
            tcp_stream,
        }
    }
}

pub struct Socket {
    addr: SocketAddr,

    packet_sender: PacketSender,
    event_receiver: Receiver<SocketEvent>,
}

impl Socket {
    pub fn new(addr: SocketAddr) -> Result<Self> {
        let received_connections = Arc::new(Mutex::new(Vec::new()));
        let established_connection = Arc::new(Mutex::new(None));

        let (packet_sender, packet_receiver) = crossbeam_channel::unbounded();
        let (event_sender, event_receiver) = crossbeam_channel::unbounded();

        if addr.ip() != IpAddr::from_str("0.0.0.0").unwrap() {
            println!("Will attempt to connect to {:?}", addr);
            let connect_addr = addr;
            thread::spawn(move || Self::connect(connect_addr, established_connection));
        }

        let poll_event_sender = event_sender.clone();
        thread::spawn(move || Self::poll(packet_receiver, poll_event_sender));

        let listen_port = addr.port();
        thread::spawn(move || Self::listen(listen_port, received_connections, event_sender));

        let packet_sender = PacketSender::new(packet_sender);

        Ok(Self {
            addr,
            packet_sender,
            event_receiver,
        })
    }

    pub fn packet_sender(&mut self) -> &mut PacketSender {
        &mut self.packet_sender
    }

    pub fn event_receiver(&mut self) -> &mut Receiver<SocketEvent> {
        &mut self.event_receiver
    }

    // Try to connect to our target address
    fn connect(addr: SocketAddr, established_connection: Arc<Mutex<Option<Connection>>>) {
        loop {
            if let Ok(mut connection) = established_connection.lock() {
                if connection.is_none() {
                    // Try to connect if we're not connected yet
                    if let Ok(tcp_stream) = TcpStream::connect(addr) {
                        *connection = Some(Connection::new(tcp_stream));
                    }
                }
            }

            thread::sleep(Duration::from_millis(500));
        }
    }

    // Listen and accept any incoming connections
    fn listen(
        port: u16,
        received_connections: Arc<Mutex<Vec<Connection>>>,
        event_sender: Sender<SocketEvent>,
    ) -> Result<()> {
        let tcp_listener =
            TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port))?;

        for stream in tcp_listener.incoming().flatten() {
            if let Ok(mut received_connections) = received_connections.lock() {
                let peer_addr = stream.peer_addr()?;
                received_connections.push(Connection::new(stream));

                event_sender.try_send(SocketEvent::Connect(peer_addr))?;
            }
        }

        Ok(())
    }

    fn poll(packet_receiver: Receiver<Packet>, event_sender: Sender<SocketEvent>) {
        loop {
            // DO STUFF
        }
    }
}
