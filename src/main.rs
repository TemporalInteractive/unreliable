use std::{
    net::UdpSocket,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Result;

fn nanos() -> u128 {
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    since_the_epoch.as_nanos()
}

fn main() -> Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:2000")?;

    loop {
        let mut buf = [0; std::mem::size_of::<u128>()];
        let (len, src) = socket.recv_from(&mut buf)?;
        let current_nanos = nanos();

        let client_nanos = *bytemuck::from_bytes::<u128>(&buf);

        let elapsed_nanos = current_nanos - client_nanos;
        println!("Elapsed {}ns", elapsed_nanos);
    }

    Ok(())
}
