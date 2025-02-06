use anyhow::Result;
use core::{
    clone::Clone,
    net::{IpAddr, Ipv4Addr, SocketAddr},
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PacketType {
    Unreliable,
    Barrier,
}

pub const MAX_PACKET_PAYLOAD_SIZE: usize = 65527;

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

    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload[0..(self.payload.len() - 4)]
    }

    pub fn timeframe(&self) -> u32 {
        *bytemuck::from_bytes::<u32>(&self.payload[(self.payload.len() - 4)..self.payload.len()])
    }

    pub fn is_barrier(&self) -> bool {
        self.ty == PacketType::Barrier
    }
}

pub struct PacketSender {
    packet_sender: Sender<Packet>,
    timeframe: Arc<AtomicU32>,
}

impl PacketSender {
    fn new(packet_sender: Sender<Packet>, timeframe: Arc<AtomicU32>) -> Self {
        Self {
            packet_sender,
            timeframe,
        }
    }

    pub fn send_unreliable(&mut self, addr: SocketAddr, mut payload: Vec<u8>) -> Result<()> {
        let timeframe = self.timeframe.load(Ordering::SeqCst);

        // Timeframe is added to the end of the message
        // This will hopefully avoid any big allocations as vectors allocate a bit more than they actually use
        payload.append(&mut bytemuck::bytes_of(&timeframe).to_vec());
        let packet = Packet::unreliable(addr, payload);

        self.packet_sender.try_send(packet)?;
        Ok(())
    }

    pub fn send_barrier(&mut self, addr: SocketAddr, mut payload: Vec<u8>) -> Result<()> {
        let timeframe = self.timeframe.load(Ordering::SeqCst);

        payload.append(&mut bytemuck::bytes_of(&timeframe).to_vec());

        let mut final_payload = Vec::with_capacity(payload.len() + 4);
        // Add payload length to the start of the message, as udp doesn't preserve message boundries
        final_payload.append(&mut bytemuck::bytes_of(&(payload.len() as u32)).to_vec());
        final_payload.append(&mut payload);

        let packet = Packet::barrier(addr, final_payload);

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
        tcp_stream.set_nonblocking(true).unwrap();
        Self { tcp_stream }
    }
}

pub struct Socket {
    packet_sender: PacketSender,
    event_receiver: Receiver<SocketEvent>,
    timeframe: Arc<AtomicU32>,
}

impl Socket {
    pub fn new(connect_addr: Option<SocketAddr>, receive_port: u16) -> Result<Self> {
        let received_connections = Arc::new(Mutex::new(HashMap::new()));
        let established_connection = Arc::new(Mutex::new(None));
        let timeframe = Arc::new(AtomicU32::new(0));

        let (packet_sender, packet_receiver) = crossbeam_channel::unbounded();
        let (event_sender, event_receiver) = crossbeam_channel::unbounded();

        if let Some(connect_addr) = connect_addr {
            let connect_established_connection = established_connection.clone();
            let connect_event_sender = event_sender.clone();
            thread::Builder::new()
                .name("Unreliable - Connect".to_owned())
                .spawn(move || {
                    Self::connect(
                        connect_addr,
                        connect_established_connection,
                        connect_event_sender,
                    )
                    .unwrap()
                })
                .unwrap();
        }

        let send_packets_event_sender = event_sender.clone();
        let send_packets_received_connections = received_connections.clone();
        let send_packets_established_connection = established_connection.clone();
        thread::Builder::new()
            .name("Unreliable - Send Packets".to_owned())
            .spawn(move || {
                Self::send_packets(
                    packet_receiver,
                    send_packets_event_sender,
                    send_packets_received_connections,
                    send_packets_established_connection,
                )
                .unwrap()
            })
            .unwrap();

        let receive_barriers_timeframe = timeframe.clone();
        let receive_barriers_received_connections = received_connections.clone();
        let receive_barriers_established_connection = established_connection.clone();
        let receive_barriers_event_sender = event_sender.clone();
        thread::Builder::new()
            .name("Unreliable - Receive Barriers".to_owned())
            .spawn(move || {
                Self::receive_barriers(
                    receive_barriers_timeframe,
                    receive_barriers_received_connections,
                    receive_barriers_established_connection,
                    receive_barriers_event_sender,
                )
            })
            .unwrap();

        let receive_unreliable_packets_port = receive_port;
        let receive_unreliable_packets_timeframe = timeframe.clone();
        let receive_unreliable_packets_event_sender = event_sender.clone();
        thread::Builder::new()
            .name("Unreliable - Receive Unreliable Packets".to_owned())
            .spawn(move || {
                Self::receive_unreliable_packets(
                    receive_unreliable_packets_port,
                    receive_unreliable_packets_timeframe,
                    receive_unreliable_packets_event_sender,
                )
            })
            .unwrap();

        let listen_port = receive_port;
        thread::Builder::new()
            .name("Unreliable - Listen".to_owned())
            .spawn(move || Self::listen(listen_port, received_connections, event_sender).unwrap())
            .unwrap();

        let packet_sender = PacketSender::new(packet_sender, timeframe.clone());

        Ok(Self {
            packet_sender,
            event_receiver,
            timeframe,
        })
    }

    pub fn packet_sender(&mut self) -> &mut PacketSender {
        &mut self.packet_sender
    }

    pub fn event_receiver(&mut self) -> &mut Receiver<SocketEvent> {
        &mut self.event_receiver
    }

    pub fn barrier(&mut self) -> Arc<AtomicU32> {
        self.timeframe.clone()
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
                    if let Ok(tcp_stream) =
                        TcpStream::connect_timeout(&addr, Duration::from_millis(100))
                    {
                        *connection = Some(Connection::new(tcp_stream));
                        event_sender.try_send(SocketEvent::Connect(addr))?;
                    }
                }
            }

            thread::yield_now();
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
                    while let Ok(packet) = packet_receiver.try_recv() {
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

    fn receive_unreliable_packets(
        port: u16,
        timeframe: Arc<AtomicU32>,
        event_sender: Sender<SocketEvent>,
    ) -> Result<()> {
        let udp_socket =
            UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port)).unwrap();

        let mut buf = vec![0u8; u16::MAX as usize];

        loop {
            if let Ok((len, src)) = udp_socket.recv_from(&mut buf) {
                let packet_timeframe = *bytemuck::from_bytes::<u32>(&buf[(len - 4)..len]);

                if packet_timeframe == timeframe.load(Ordering::SeqCst) {
                    let packet = Packet {
                        addr: src,
                        payload: buf[0..len].to_vec().into_boxed_slice(),
                        ty: PacketType::Unreliable,
                    };

                    event_sender.try_send(SocketEvent::Packet(packet))?;
                }
            }

            thread::yield_now();
        }
    }

    fn receive_barrier(
        connection: &mut Connection,
        timeframe: &Arc<AtomicU32>,
        event_sender: &mut Sender<SocketEvent>,
        buf: &mut [u8],
    ) -> Result<()> {
        if let Ok(total_len) = connection.tcp_stream.read(buf) {
            if total_len > 0 {
                let mut reader = 0;

                // Read all packets
                while reader < total_len {
                    // Receive packet length as TCP doesn't preserve boundries
                    let len = *bytemuck::from_bytes::<u32>(&buf[reader..(reader + 4)]) as usize;
                    reader += 4;

                    // Read barrier timeframe from end of packet
                    let barrier_timeframe =
                        *bytemuck::from_bytes::<u32>(&buf[(reader + len - 4)..(reader + len)]);
                    if barrier_timeframe < timeframe.load(Ordering::SeqCst) {
                        panic!("Barriers can only increase over time.");
                    }
                    timeframe.store(barrier_timeframe, Ordering::SeqCst);

                    // Read payload, timeframe is included
                    let payload = buf[reader..(reader + len)].to_vec().into_boxed_slice();
                    reader += len;

                    let packet = Packet {
                        addr: connection.tcp_stream.peer_addr().unwrap(),
                        payload,
                        ty: PacketType::Barrier,
                    };

                    event_sender.try_send(SocketEvent::Packet(packet))?;
                }
            }
        }

        Ok(())
    }

    fn receive_barriers(
        timeframe: Arc<AtomicU32>,
        received_connections: Arc<Mutex<HashMap<SocketAddr, Connection>>>,
        established_connection: Arc<Mutex<Option<Connection>>>,
        mut event_sender: Sender<SocketEvent>,
    ) -> Result<()> {
        let mut buf = vec![0u8; u16::MAX as usize];

        loop {
            if let Ok(mut received_connections) = received_connections.lock() {
                for connection in received_connections.values_mut() {
                    Self::receive_barrier(connection, &timeframe, &mut event_sender, &mut buf)?;
                }
            }

            if let Ok(mut established_connection) = established_connection.lock() {
                if let Some(connection) = established_connection.as_mut() {
                    Self::receive_barrier(connection, &timeframe, &mut event_sender, &mut buf)?;
                }
            }

            thread::yield_now();
        }
    }
}
