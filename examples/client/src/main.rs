use core::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use unreliable::*;

fn main() {
    let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(169, 254, 187, 239), 2344));

    let mut socket = Socket::new(addr).unwrap();

    loop {
        if let Ok(socket_event) = socket.event_receiver().recv() {
            match socket_event {
                SocketEvent::Packet(packet) => {}
                SocketEvent::Connect(addr) => {
                    println!("Connect: {:?}", addr);
                    socket.packet_sender().send_barrier(addr).unwrap();
                }
                SocketEvent::Disconnect(addr) => {
                    println!("Disconnect: {:?}", addr);
                }
            }
        }
    }
}
