//! Make every packaged icon from the app icon SVG:
//! `cargo run -p calcite-core --example brand_icons -- <app-icon.svg> <out dir>`
//! writes `Calcite.iconset/` (for `iconutil`), `calcite.ico` and
//! `calcite-{256,512}.png`.

use std::path::Path;

use image::RgbaImage;
use resvg::{tiny_skia, usvg};

fn render(tree: &usvg::Tree, size: u32) -> RgbaImage {
    let scale = size as f32 / tree.size().width().max(tree.size().height());
    let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("size");
    resvg::render(
        tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia stores premultiplied alpha; PNG and ICO want straight alpha.
    let mut image = RgbaImage::new(size, size);
    for (pixel, out) in pixmap.pixels().iter().zip(image.pixels_mut()) {
        let c = pixel.demultiply();
        *out = image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    image
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [svg, out] = args.as_slice() else {
        eprintln!("usage: brand_icons <app-icon.svg> <out dir>");
        std::process::exit(2);
    };
    let tree = usvg::Tree::from_data(&std::fs::read(svg)?, &usvg::Options::default())?;
    let out = Path::new(out);

    // macOS: the iconset iconutil turns into Calcite.icns.
    let iconset = out.join("Calcite.iconset");
    std::fs::create_dir_all(&iconset)?;
    for base in [16u32, 32, 128, 256, 512] {
        render(&tree, base).save(iconset.join(format!("icon_{base}x{base}.png")))?;
        render(&tree, base * 2).save(iconset.join(format!("icon_{base}x{base}@2x.png")))?;
    }

    // Windows: one .ico with the sizes Explorer and the taskbar ask for.
    let frames: Vec<image::codecs::ico::IcoFrame> = [16u32, 24, 32, 48, 64, 128, 256]
        .into_iter()
        .map(|size| {
            let image = render(&tree, size);
            image::codecs::ico::IcoFrame::as_png(
                image.as_raw(),
                size,
                size,
                image::ExtendedColorType::Rgba8,
            )
        })
        .collect::<Result<_, _>>()?;
    image::codecs::ico::IcoEncoder::new(std::fs::File::create(out.join("calcite.ico"))?)
        .encode_images(&frames)?;

    // Linux: plain PNGs for the desktop entry and AppImage.
    for size in [256u32, 512] {
        render(&tree, size).save(out.join(format!("calcite-{size}.png")))?;
    }
    println!("icons written to {}", out.display());
    Ok(())
}
