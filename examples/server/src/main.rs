use unreliable::*;

fn main() {
    let mut socket = Socket::new(None, 2345).unwrap();

    loop {
        if let Ok(socket_event) = socket.event_receiver().recv() {
            match socket_event {
                SocketEvent::Packet(_packet) => {}
                SocketEvent::Connect(addr) => {
                    println!("Connect: {:?}", addr);
                }
                SocketEvent::Disconnect(addr) => {
                    println!("Disconnect: {:?}", addr);
                }
            }
        }
    }
}
