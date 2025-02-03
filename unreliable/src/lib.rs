use anyhow::Result;
use core::{
    clone::Clone,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    str::FromStr,
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};
use crossbeam_channel::{Receiver, Sender};
use std::{
    collections::HashMap,
    io::{Read, Write},
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
    tcp_stream: TcpStream,
}

impl Connection {
    fn new(tcp_stream: TcpStream) -> Self {
        Self { tcp_stream }
    }
}

pub struct Socket {
    packet_sender: PacketSender,
    event_receiver: Receiver<SocketEvent>,
}

impl Socket {
    pub fn new(addr: SocketAddr) -> Result<Self> {
        let received_connections = Arc::new(Mutex::new(HashMap::new()));
        let established_connection = Arc::new(Mutex::new(None));
        let timeframe = Arc::new(AtomicU32::new(0));

        let (packet_sender, packet_receiver) = crossbeam_channel::unbounded();
        let (event_sender, event_receiver) = crossbeam_channel::unbounded();

        if addr.ip() != IpAddr::from_str("0.0.0.0").unwrap() {
            println!("Will attempt to connect to {:?}", addr);
            let connect_addr = addr;
            let connect_established_connection = established_connection.clone();
            let connect_event_sender = event_sender.clone();
            thread::spawn(move || {
                Self::connect(
                    connect_addr,
                    connect_established_connection,
                    connect_event_sender,
                )
            });
        }

        let send_packets_event_sender = event_sender.clone();
        let send_packets_received_connections = received_connections.clone();
        let send_packets_established_connection = established_connection.clone();
        thread::spawn(move || {
            Self::send_packets(
                packet_receiver,
                send_packets_event_sender,
                send_packets_received_connections,
                send_packets_established_connection,
            )
        });

        let receive_barriers_timeframe = timeframe.clone();
        let receive_barriers_received_connections = received_connections.clone();
        let receive_barriers_established_connection = established_connection.clone();
        thread::spawn(move || {
            Self::receive_barriers(
                receive_barriers_timeframe,
                receive_barriers_received_connections,
                receive_barriers_established_connection,
            )
        });

        let listen_port = addr.port();
        thread::spawn(move || Self::listen(listen_port, received_connections, event_sender));

        let packet_sender = PacketSender::new(packet_sender);

        Ok(Self {
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
    fn connect(
        addr: SocketAddr,
        established_connection: Arc<Mutex<Option<Connection>>>,
        event_sender: Sender<SocketEvent>,
    ) -> Result<()> {
        loop {
            if let Ok(mut connection) = established_connection.lock() {
                if connection.is_none() {
                    // Try to connect if we're not connected yet
                    if let Ok(tcp_stream) = TcpStream::connect(addr) {
                        *connection = Some(Connection::new(tcp_stream));
                        event_sender.try_send(SocketEvent::Connect(addr))?;
                    }
                }
            }

            thread::sleep(Duration::from_millis(100));
        }
    }

    // Listen and accept any incoming connections
    fn listen(
        port: u16,
        received_connections: Arc<Mutex<HashMap<SocketAddr, Connection>>>,
        event_sender: Sender<SocketEvent>,
    ) -> Result<()> {
        let tcp_listener =
            TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port))?;

        for stream in tcp_listener.incoming().flatten() {
            if let Ok(mut received_connections) = received_connections.lock() {
                let peer_addr = stream.peer_addr()?;
                received_connections.insert(peer_addr, Connection::new(stream));

                event_sender.try_send(SocketEvent::Connect(peer_addr))?;
            }
        }

        Ok(())
    }

    fn send_packets(
        packet_receiver: Receiver<Packet>,
        event_sender: Sender<SocketEvent>,
        received_connections: Arc<Mutex<HashMap<SocketAddr, Connection>>>,
        established_connection: Arc<Mutex<Option<Connection>>>,
    ) -> Result<()> {
        let udp_socket = UdpSocket::bind("0.0.0.0:0").unwrap();

        loop {
            if let Ok(mut received_connections) = received_connections.lock() {
                if let Ok(mut established_connection) = established_connection.lock() {
                    // Receive a packet to send out to an address
                    if let Ok(packet) = packet_receiver.try_recv() {
                        match packet.ty {
                            // Unreliable packets go over udp
                            PacketType::Unreliable => {
                                if udp_socket.send_to(&packet.payload, packet.addr).is_err() {
                                    if received_connections.remove(&packet.addr).is_some() {
                                        event_sender
                                            .try_send(SocketEvent::Disconnect(packet.addr))?;
                                    }

                                    if let Some(connection) = established_connection.as_ref() {
                                        if connection.tcp_stream.peer_addr()? == packet.addr {
                                            *established_connection = None;
                                        }
                                    }
                                }
                            }
                            // Barriers go over tcp
                            PacketType::Barrier => {
                                if let Some(connection) = received_connections.get_mut(&packet.addr)
                                {
                                    if connection.tcp_stream.write(&packet.payload).is_err() {
                                        received_connections.remove(&packet.addr);
                                        event_sender
                                            .try_send(SocketEvent::Disconnect(packet.addr))?;
                                    }
                                } else if let Some(connection) = established_connection.as_mut() {
                                    if connection.tcp_stream.peer_addr()? == packet.addr
                                        && connection.tcp_stream.write(&packet.payload).is_err()
                                    {
                                        *established_connection = None;
                                        event_sender
                                            .try_send(SocketEvent::Disconnect(packet.addr))?;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            thread::yield_now();
        }
    }

    fn receive_barrier(connection: &mut Connection, timeframe: &Arc<AtomicU32>) {
        let mut buf = [0u8; 4];
        if let Ok(len) = connection.tcp_stream.peek(&mut buf) {
            if len > 0 {
                if let Ok(_len) = connection.tcp_stream.read(&mut buf) {
                    let barrier_timeframe = *bytemuck::from_bytes::<u32>(buf.as_ref());
                    timeframe.store(barrier_timeframe, Ordering::SeqCst);
                    println!("Barrier: {}!", barrier_timeframe);
                }
            }
        }
    }

    fn receive_barriers(
        timeframe: Arc<AtomicU32>,
        received_connections: Arc<Mutex<HashMap<SocketAddr, Connection>>>,
        established_connection: Arc<Mutex<Option<Connection>>>,
    ) {
        loop {
            if let Ok(mut received_connections) = received_connections.lock() {
                for connection in received_connections.values_mut() {
                    Self::receive_barrier(connection, &timeframe);
                }
            }

            if let Ok(mut established_connection) = established_connection.lock() {
                if let Some(connection) = established_connection.as_mut() {
                    Self::receive_barrier(connection, &timeframe);
                }
            }

            thread::yield_now();
        }
    }
}
