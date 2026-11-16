//! Render an SVG to a square PNG: `cargo run -p calcite-core --example
//! render_svg -- in.svg out.png 1024`. Used to make the brand PNGs (app
//! and Discord icons) from the SVGs in `crates/calcite/assets/brand`.

use resvg::{tiny_skia, usvg};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output, size] = args.as_slice() else {
        eprintln!("usage: render_svg <in.svg> <out.png> <size>");
        std::process::exit(2);
    };
    let size: u32 = size.parse()?;
    let tree = usvg::Tree::from_data(&std::fs::read(input)?, &usvg::Options::default())?;
    let scale = size as f32 / tree.size().width().max(tree.size().height());
    let mut pixmap = tiny_skia::Pixmap::new(size, size).ok_or("bad size")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.save_png(output)?;
    println!("wrote {output} ({size}×{size})");
    Ok(())
}
