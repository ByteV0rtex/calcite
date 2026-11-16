//! Render a skin PNG to an image: `skin_preview <skin.png> <out.png> [slim] [yaw] [pitch] [cape.png]`.
use calcite_core::skin3d::{Model, View, normalize, render};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let skin =
        normalize(&image::open(&args[0]).expect("skin").to_rgba8()).expect("64x64 or 64x32 skin");
    let model = if args.get(2).map(String::as_str) == Some("slim") {
        Model::Slim
    } else {
        Model::Classic
    };
    let yaw = args
        .get(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(View::default().yaw);
    let pitch = args
        .get(4)
        .and_then(|s| s.parse().ok())
        .unwrap_or(View::default().pitch);
    let cape = args
        .get(5)
        .map(|p| image::open(p).expect("cape").to_rgba8());
    let start = std::time::Instant::now();
    let img = render(&skin, cape.as_ref(), model, View { yaw, pitch }, 260, 400);
    eprintln!("rendered in {:?}", start.elapsed());
    img.save(&args[1]).expect("save");
}
