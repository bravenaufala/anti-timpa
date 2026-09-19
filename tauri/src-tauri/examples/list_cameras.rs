//! Prints the cameras visible to nokhwa on this machine, and verifies each one
//! can actually deliver a frame.
//!
//! Run with:
//! ```text
//! cargo run --example list_cameras --features desktop-camera
//! ```
//!
//! Exists because "camera not found" is otherwise indistinguishable from
//! "wrong index" or "permission denied", and those need different fixes. On a
//! typical Linux laptop the camera is often NOT `/dev/video0`, and UVC devices
//! expose extra nodes that open but never deliver video — both of which this
//! makes obvious.

use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{ApiBackend, CameraIndex, RequestedFormat, RequestedFormatType};
use nokhwa::Camera;

fn main() {
    println!("== Kueri daftar kamera (ApiBackend::Auto) ==");
    let devices = match nokhwa::query(ApiBackend::Auto) {
        Ok(d) if d.is_empty() => {
            println!("  (kosong) nokhwa tidak melihat kamera apa pun.");
            Vec::new()
        }
        Ok(d) => {
            for info in &d {
                println!(
                    "  index={:?}\n    nama      : {}\n    deskripsi : {}\n    misc      : {}",
                    info.index(),
                    info.human_name(),
                    info.description(),
                    info.misc()
                );
            }
            d
        }
        Err(e) => {
            println!("  GAGAL kueri: {e}");
            Vec::new()
        }
    };

    let indices: Vec<u32> = devices
        .iter()
        .filter_map(|i| match i.index() {
            CameraIndex::Index(n) => Some(*n),
            CameraIndex::String(_) => None,
        })
        .collect();

    println!("\n== Uji buka + ambil 1 frame per device ==");
    if indices.is_empty() {
        println!("  tidak ada index untuk diuji.");
        return;
    }

    for index in indices {
        // Highest frame rate at any resolution, matching what the app requests.
        let format =
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);

        print!("  index={index}: ");
        let mut camera = match Camera::new(CameraIndex::Index(index), format) {
            Ok(c) => {
                print!("Camera::new OK ({:?}) ", c.resolution());
                c
            }
            Err(e) => {
                println!("Camera::new GAGAL: {e}");
                continue;
            }
        };

        if let Err(e) = camera.open_stream() {
            println!("open_stream GAGAL: {e}");
            continue;
        }
        print!("stream OK ");

        // A device can open and stream yet still never deliver frames — which
        // is exactly what a UVC metadata node does. Grabbing a frame is the
        // only conclusive check.
        match camera.frame() {
            Ok(buf) => println!(
                "FRAME OK {}x{} format={}",
                buf.resolution().width_x,
                buf.resolution().height_y,
                buf.source_frame_format()
            ),
            Err(e) => println!("FRAME GAGAL: {e}"),
        }

        let _ = camera.stop_stream();
    }
}
