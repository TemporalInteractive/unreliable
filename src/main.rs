use std::{
    net::UdpSocket,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Result;

use rayon::prelude::*;

fn nanos() -> u128 {
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    since_the_epoch.as_nanos()
}

//const MAX_PACKET_SIZE: usize = 8972; //65507;
const MAX_PACKET_SIZE: usize = 65507;

const TEST_ITERS: u128 = 100;

fn main() -> Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;

    let pixel_bytes = (1024 * 512 * 3) as u32;
    let required_packages = pixel_bytes.div_ceil(MAX_PACKET_SIZE as u32);

    let nanos = (0..TEST_ITERS)
        .collect::<Vec<u128>>()
        .par_iter()
        .map(|i| {
            let mut buf = [0u8; MAX_PACKET_SIZE];

            for _ in 0..required_packages {
                socket
                    .send_to(&buf, "169.254.187.239:2000")
                    .expect("Error on send");
            }
        })
        .collect::<Vec<_>>();

    Ok(())
}
