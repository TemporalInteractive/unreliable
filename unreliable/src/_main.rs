use std::{
    net::UdpSocket,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use image::codecs::jpeg::JpegEncoder;
use turbojpeg::Image;

fn nanos() -> u128 {
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    since_the_epoch.as_nanos()
}

//const MAX_PACKET_SIZE: usize = 8972;
const MAX_PACKET_SIZE: usize = 65507;

fn main() -> Result<()> {
    let imgx = 1920;
    let imgy = 1080;

    let pixels = vec![0u8; imgx * imgy * 3];

    let mut total = 0;

    for _ in 0..100 {
        let start = Instant::now();

        let image = Image {
            pixels: pixels.as_slice(),
            width: imgx,
            height: imgy,
            pitch: imgx * 3,
            format: turbojpeg::PixelFormat::RGB,
        };
        let len = turbojpeg::compress(image, 95, turbojpeg::Subsamp::Sub2x2)
            .unwrap()
            .len();

        let elapsed = start.elapsed().as_nanos();

        println!("{}ns / {}ms", elapsed, start.elapsed().as_millis());
        total += elapsed;

        let reduction = len as f32 / (imgx * imgy) as f32;
        println!("Reduction: {}", reduction);
    }

    let avg = total / 100;
    println!("AVG - {}ns / {}ms", avg, avg as f32 / 1000000.0);

    //let reduction = (imgx * imgy) as f32 / default.len() as f32;
    //println!("Reduction: {}", reduction);

    Ok(())

    // fn main() -> Result<()> {
    //     let imgx = 1920;
    //     let imgy = 1080;

    //     let scalex = 3.0 / imgx as f32;
    //     let scaley = 3.0 / imgy as f32;

    //     // Create a new ImgBuf with width: imgx and height: imgy
    //     let mut imgbuf = image::ImageBuffer::new(imgx, imgy);

    //     // Iterate over the coordinates and pixels of the image
    //     for (x, y, pixel) in imgbuf.enumerate_pixels_mut() {
    //         let r = (0.3 * x as f32) as u8;
    //         let b = (0.3 * y as f32) as u8;
    //         *pixel = image::Rgb([r, 0, b]);
    //     }

    //     let mut default = vec![];

    //     let mut total = 0;

    //     for _ in 0..100 {
    //         let start = Instant::now();

    //         let encoder = JpegEncoder::new_with_quality(&mut default, 20);
    //         imgbuf.write_with_encoder(encoder).unwrap();
    //         let elapsed = start.elapsed().as_nanos();

    //         println!("{}ns / {}ms", elapsed, start.elapsed().as_millis());
    //         total += elapsed;
    //     }

    //     let avg = total / 100;
    //     println!("AVG - {}ns / {}ms", avg, avg as f32 / 1000000.0);

    //     let reduction = (imgx * imgy) as f32 / default.len() as f32;
    //     println!("Reduction: {}", reduction);

    //     imgbuf.save("fractal.jpeg").unwrap();

    //     Ok(())

    // let socket = UdpSocket::bind("0.0.0.0:2000")?;

    // println!("Started...");

    // let pixel_bytes = (1920 * 1080 * 3) as u32;
    // let required_packages = pixel_bytes.div_ceil(MAX_PACKET_SIZE as u32);

    // let mut buf = [0; MAX_PACKET_SIZE];

    // let mut prev_recv = None;

    // let mut recv_time = 0;
    // let mut recv_count = 0;

    // loop {
    //     for _ in 0..1000 {
    //         let (_, src) = socket.recv_from(&mut buf)?;
    //     }

    //     let current = nanos();
    //     if let Some(prev_recv) = &prev_recv {
    //         recv_time += current - prev_recv;
    //         recv_count += 1;
    //     }
    //     prev_recv = Some(current);

    //     if recv_count > 0 {
    //         let ns = (recv_time * required_packages as u128) / (recv_count * 1000);
    //         println!("expected avg {}ns / {}ms", ns, (ns as f32) / 1000000.0);
    //     }

    //     recv_count = 0;
    //     recv_time = 0;
    // }
}
