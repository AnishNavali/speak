fn main() {
    for key in ["SPEAK_RELEASE_REPOSITORY", "SPEAK_UPDATER_PUBLIC_KEY"] {
        println!("cargo:rerun-if-env-changed={key}");
        if let Ok(value) = std::env::var(key) {
            println!("cargo:rustc-env={key}={}", value.trim());
        }
    }
    println!(
        "cargo:rustc-env=SPEAK_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
    std::fs::create_dir_all("icons").unwrap();
    if !std::path::Path::new("icons/icon.png").exists() {
        let mut pixels = vec![247u8; 256 * 256 * 4];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        for (x, height) in [(48, 64), (96, 144), (144, 192), (192, 96)] {
            for y in (256 - height) / 2..(256 + height) / 2 {
                for dx in 0..20 {
                    pixels[(y * 256 + x + dx) * 4..(y * 256 + x + dx) * 4 + 4]
                        .copy_from_slice(&[82, 106, 78, 255]);
                }
            }
        }
        let mut image = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut image, 256, 256);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&pixels)
                .unwrap();
        }
        std::fs::write("icons/icon.png", &image).unwrap();
        let mut ico = vec![0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 32, 0];
        ico.extend_from_slice(&(image.len() as u32).to_le_bytes());
        ico.extend_from_slice(&22u32.to_le_bytes());
        ico.extend(image);
        std::fs::write("icons/icon.ico", ico).unwrap();
    }
    tauri_build::build();
}
