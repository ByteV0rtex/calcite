//! A tiny software renderer for Minecraft player skins.
//!
//! The player is built from textured boxes (head, body, arms, legs, their
//! outer layers, and an optional cape), projected orthographically, shaded
//! per face, and drawn with a z-buffer at 2× size for smooth edges. It only
//! runs when the view changes, so a preview costs nothing while idle.

use image::{Rgba, RgbaImage};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    /// Arms 4 pixels wide.
    Classic,
    /// Arms 3 pixels wide.
    Slim,
}

/// Camera angles in radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Turn around the vertical axis; 0 faces the player head-on.
    pub yaw: f32,
    /// Tilt; positive looks down at the player.
    pub pitch: f32,
}

impl Default for View {
    fn default() -> Self {
        View {
            yaw: -0.5,
            pitch: 0.18,
        }
    }
}

type V3 = [f32; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A rotation about a pivot, as an affine map `p ↦ m·p + offset`.
#[derive(Clone, Copy)]
struct Turn {
    m: [V3; 3],
    offset: V3,
}

impl Turn {
    const NONE: Turn = Turn {
        m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        offset: [0.0; 3],
    };
    fn around(m: [V3; 3], pivot: V3) -> Turn {
        let t = Turn {
            m,
            offset: [0.0; 3],
        };
        Turn {
            m,
            offset: sub(pivot, t.vec(pivot)),
        }
    }
    fn x(angle: f32, pivot: V3) -> Turn {
        let (s, c) = angle.sin_cos();
        Turn::around([[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]], pivot)
    }
    fn y(angle: f32, pivot: V3) -> Turn {
        let (s, c) = angle.sin_cos();
        Turn::around([[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]], pivot)
    }
    fn vec(&self, v: V3) -> V3 {
        [dot(self.m[0], v), dot(self.m[1], v), dot(self.m[2], v)]
    }
    fn point(&self, p: V3) -> V3 {
        add(self.vec(p), self.offset)
    }
    /// Apply `first`, then `self`.
    fn then(self, first: Turn) -> Turn {
        let col = |j: usize| self.vec([first.m[0][j], first.m[1][j], first.m[2][j]]);
        let (c0, c1, c2) = (col(0), col(1), col(2));
        Turn {
            m: [
                [c0[0], c1[0], c2[0]],
                [c0[1], c1[1], c2[1]],
                [c0[2], c1[2], c2[2]],
            ],
            offset: add(self.vec(first.offset), self.offset),
        }
    }
}

/// One textured face: a parallelogram `origin + s·S + t·T`, s,t ∈ [0,1).
struct Face {
    origin: V3,
    s: V3,
    t: V3,
    normal: V3,
    /// Texture rectangle (u, v, width, height) in texels.
    rect: (u32, u32, u32, u32),
    texture: usize,
    /// Outer layers skip transparent texels; the base layer is opaque.
    overlay: bool,
}

/// A box with Minecraft's standard texture layout at (u, v).
struct Cuboid {
    min: V3,
    size: V3,
    uv: (u32, u32),
    inflate: f32,
    overlay: bool,
    texture: usize,
    turn: Turn,
}

impl Cuboid {
    fn faces(&self, out: &mut Vec<Face>) {
        let (w, h, d) = (
            self.size[0] as u32,
            self.size[1] as u32,
            self.size[2] as u32,
        );
        let (u, v) = self.uv;
        let i = self.inflate;
        let [x0, y0, z0] = [self.min[0] - i, self.min[1] - i, self.min[2] - i];
        let [x1, y1, z1] = [
            self.min[0] + self.size[0] + i,
            self.min[1] + self.size[1] + i,
            self.min[2] + self.size[2] + i,
        ];
        let (dx, dy, dz) = (x1 - x0, y1 - y0, z1 - z0);
        let faces = [
            // front (+z), back (-z), right (-x), left (+x), top (+y), bottom (-y)
            (
                [x0, y1, z1],
                [dx, 0.0, 0.0],
                [0.0, -dy, 0.0],
                [0.0, 0.0, 1.0],
                (u + d, v + d, w, h),
            ),
            (
                [x1, y1, z0],
                [-dx, 0.0, 0.0],
                [0.0, -dy, 0.0],
                [0.0, 0.0, -1.0],
                (u + 2 * d + w, v + d, w, h),
            ),
            (
                [x0, y1, z0],
                [0.0, 0.0, dz],
                [0.0, -dy, 0.0],
                [-1.0, 0.0, 0.0],
                (u, v + d, d, h),
            ),
            (
                [x1, y1, z1],
                [0.0, 0.0, -dz],
                [0.0, -dy, 0.0],
                [1.0, 0.0, 0.0],
                (u + d + w, v + d, d, h),
            ),
            (
                [x0, y1, z0],
                [dx, 0.0, 0.0],
                [0.0, 0.0, dz],
                [0.0, 1.0, 0.0],
                (u + d, v, w, d),
            ),
            (
                [x0, y0, z1],
                [dx, 0.0, 0.0],
                [0.0, 0.0, -dz],
                [0.0, -1.0, 0.0],
                (u + d + w, v, w, d),
            ),
        ];
        for (origin, s, t, normal, rect) in faces {
            out.push(Face {
                origin: self.turn.point(origin),
                s: self.turn.vec(s),
                t: self.turn.vec(t),
                normal: self.turn.vec(normal),
                rect,
                texture: self.texture,
                overlay: self.overlay,
            });
        }
    }
}

/// Bring a skin to the 64×64 layout. Old 64×32 skins get mirrored copies of
/// the right arm and leg, the way Minecraft does it. `None` if it isn't a
/// skin texture.
/// The player's face (with the hat layer on top), `size`×`size` with crisp
/// pixels, for avatars. Takes a normalized 64×64 skin.
pub fn face(skin: &RgbaImage, size: u32) -> RgbaImage {
    let mut face = RgbaImage::new(8, 8);
    for y in 0..8 {
        for x in 0..8 {
            let base = *skin.get_pixel(8 + x, 8 + y);
            let hat = *skin.get_pixel(40 + x, 8 + y);
            // Blend the hat over the face by its alpha.
            let a = hat[3] as u32;
            let mix = |b: u8, h: u8| ((h as u32 * a + b as u32 * (255 - a)) / 255) as u8;
            face.put_pixel(
                x,
                y,
                image::Rgba([
                    mix(base[0], hat[0]),
                    mix(base[1], hat[1]),
                    mix(base[2], hat[2]),
                    255,
                ]),
            );
        }
    }
    image::imageops::resize(&face, size, size, image::imageops::FilterType::Nearest)
}

pub fn normalize(skin: &RgbaImage) -> Option<RgbaImage> {
    match skin.dimensions() {
        (64, 64) => Some(skin.clone()),
        (64, 32) => {
            let mut out = RgbaImage::new(64, 64);
            for (x, y, p) in skin.enumerate_pixels() {
                out.put_pixel(x, y, *p);
            }
            // (from x, from y, width, height, to x, to y), mirrored horizontally.
            const COPIES: [(u32, u32, u32, u32, u32, u32); 12] = [
                (4, 16, 4, 4, 20, 48),
                (8, 16, 4, 4, 24, 48),
                (0, 20, 4, 12, 24, 52),
                (4, 20, 4, 12, 20, 52),
                (8, 20, 4, 12, 16, 52),
                (12, 20, 4, 12, 28, 52),
                (44, 16, 4, 4, 36, 48),
                (48, 16, 4, 4, 40, 48),
                (40, 20, 4, 12, 40, 52),
                (44, 20, 4, 12, 36, 52),
                (48, 20, 4, 12, 32, 52),
                (52, 20, 4, 12, 44, 52),
            ];
            for (sx, sy, w, h, dx, dy) in COPIES {
                for y in 0..h {
                    for x in 0..w {
                        let p = *skin.get_pixel(sx + x, sy + y);
                        out.put_pixel(dx + (w - 1 - x), dy + y, p);
                    }
                }
            }
            Some(out)
        }
        _ => None,
    }
}

/// Guess the arm width: slim skins leave the last two columns of the right
/// arm's back (x 54–55, y 20–31) empty.
pub fn looks_slim(skin: &RgbaImage) -> bool {
    skin.height() == 64
        && (20..32).all(|y| skin.get_pixel(54, y)[3] == 0 && skin.get_pixel(55, y)[3] == 0)
}

fn player(model: Model, cape: bool) -> Vec<Cuboid> {
    let arm = if model == Model::Slim { 3.0 } else { 4.0 };
    let part = |min: V3, size: V3, uv: (u32, u32), inflate: f32, overlay: bool| Cuboid {
        min,
        size,
        uv,
        inflate,
        overlay,
        texture: 0,
        turn: Turn::NONE,
    };
    let mut boxes = vec![
        part([-4.0, 24.0, -4.0], [8.0, 8.0, 8.0], (0, 0), 0.0, false),
        part([-4.0, 12.0, -2.0], [8.0, 12.0, 4.0], (16, 16), 0.0, false),
        part(
            [-4.0 - arm, 12.0, -2.0],
            [arm, 12.0, 4.0],
            (40, 16),
            0.0,
            false,
        ),
        part([4.0, 12.0, -2.0], [arm, 12.0, 4.0], (32, 48), 0.0, false),
        part([-4.0, 0.0, -2.0], [4.0, 12.0, 4.0], (0, 16), 0.0, false),
        part([0.0, 0.0, -2.0], [4.0, 12.0, 4.0], (16, 48), 0.0, false),
        // Outer layers.
        part([-4.0, 24.0, -4.0], [8.0, 8.0, 8.0], (32, 0), 0.5, true),
        part([-4.0, 12.0, -2.0], [8.0, 12.0, 4.0], (16, 32), 0.25, true),
        part(
            [-4.0 - arm, 12.0, -2.0],
            [arm, 12.0, 4.0],
            (40, 32),
            0.25,
            true,
        ),
        part([4.0, 12.0, -2.0], [arm, 12.0, 4.0], (48, 48), 0.25, true),
        part([-4.0, 0.0, -2.0], [4.0, 12.0, 4.0], (0, 32), 0.25, true),
        part([0.0, 0.0, -2.0], [4.0, 12.0, 4.0], (0, 48), 0.25, true),
    ];
    if cape {
        // Built facing forward, then turned around onto the back and tilted
        // so it hangs away from the body.
        let flip = Turn::y(std::f32::consts::PI, [0.0, 24.0, 0.0]);
        let hang = Turn::x(0.18, [0.0, 24.0, -2.0]);
        boxes.push(Cuboid {
            min: [-5.0, 8.0, 2.0],
            size: [10.0, 16.0, 1.0],
            uv: (0, 0),
            inflate: 0.0,
            overlay: false,
            texture: 1,
            turn: hang.then(flip),
        });
    }
    boxes
}

/// Draw the player at 2× and scale down, for smooth edges. `cape` is a
/// 64×32 cape texture. The skin should already be [`normalize`]d.
pub fn render(
    skin: &RgbaImage,
    cape: Option<&RgbaImage>,
    model: Model,
    view: View,
    width: u32,
    height: u32,
) -> RgbaImage {
    render_with(skin, cape, model, view, width, height, 2)
}

/// [`render`] with a chosen supersampling factor (1 is enough on HiDPI
/// screens, where the image is already drawn at device resolution).
pub fn render_with(
    skin: &RgbaImage,
    cape: Option<&RgbaImage>,
    model: Model,
    view: View,
    width: u32,
    height: u32,
    supersample: u32,
) -> RgbaImage {
    let ss = supersample.max(1);
    let (w, h) = (width * ss, height * ss);
    let mut color = vec![[0f32; 4]; (w * h) as usize];
    let mut depth = vec![f32::NEG_INFINITY; (w * h) as usize];

    let textures: Vec<&RgbaImage> = std::iter::once(skin).chain(cape).collect();
    let mut faces = Vec::new();
    for cuboid in player(model, cape.is_some()) {
        cuboid.faces(&mut faces);
    }

    // Camera: turn the model, then look along -z with y up.
    let cam = Turn::x(view.pitch, [0.0; 3]).then(Turn::y(view.yaw, [0.0; 3]));
    let center = [0.0, 16.0, 0.0];
    let fit = (h as f32 * 0.86 / 34.0).min(w as f32 * 0.86 / 22.0);
    let to_screen = |p: V3| -> V3 {
        let q = cam.point(sub(p, center));
        [
            w as f32 / 2.0 + q[0] * fit,
            h as f32 / 2.0 - q[1] * fit,
            q[2],
        ]
    };
    let light = {
        let l: V3 = [-0.45, 0.65, 0.62];
        scale(l, 1.0 / dot(l, l).sqrt())
    };

    for face in &faces {
        let n = cam.vec(face.normal);
        if n[2] <= 1e-4 {
            continue;
        }
        let shade = 0.62 + 0.38 * dot(n, light).max(0.0);
        let o = to_screen(face.origin);
        let a = sub(to_screen(add(face.origin, face.s)), o);
        let b = sub(to_screen(add(face.origin, face.t)), o);
        let det = a[0] * b[1] - a[1] * b[0];
        if det.abs() < 1e-6 {
            continue;
        }
        let xs = [o[0], o[0] + a[0], o[0] + b[0], o[0] + a[0] + b[0]];
        let ys = [o[1], o[1] + a[1], o[1] + b[1], o[1] + a[1] + b[1]];
        let x_lo = xs.iter().copied().fold(f32::MAX, f32::min).floor().max(0.0) as u32;
        let x_hi = xs
            .iter()
            .copied()
            .fold(f32::MIN, f32::max)
            .ceil()
            .min(w as f32) as u32;
        let y_lo = ys.iter().copied().fold(f32::MAX, f32::min).floor().max(0.0) as u32;
        let y_hi = ys
            .iter()
            .copied()
            .fold(f32::MIN, f32::max)
            .ceil()
            .min(h as f32) as u32;
        let tex = textures[face.texture];
        let (u0, v0, tw, th) = face.rect;
        // Texture coordinates scale with the texture (capes can be HD).
        let (kx, ky) = if face.texture == 1 {
            (tex.width() as f32 / 64.0, tex.height() as f32 / 32.0)
        } else {
            (1.0, 1.0)
        };
        for py in y_lo..y_hi {
            for px in x_lo..x_hi {
                let qx = px as f32 + 0.5 - o[0];
                let qy = py as f32 + 0.5 - o[1];
                let s = (qx * b[1] - qy * b[0]) / det;
                let t = (a[0] * qy - a[1] * qx) / det;
                if !(0.0..1.0).contains(&s) || !(0.0..1.0).contains(&t) {
                    continue;
                }
                let z = o[2] + s * a[2] + t * b[2];
                let ix = (py * w + px) as usize;
                if z <= depth[ix] {
                    continue;
                }
                let tu = ((u0 as f32 + s * tw as f32) * kx) as u32;
                let tv = ((v0 as f32 + t * th as f32) * ky) as u32;
                if tu >= tex.width() || tv >= tex.height() {
                    continue;
                }
                let Rgba([r, g, bl, al]) = *tex.get_pixel(tu, tv);
                if face.overlay && al < 128 {
                    continue;
                }
                depth[ix] = z;
                color[ix] = [r as f32 * shade, g as f32 * shade, bl as f32 * shade, 255.0];
            }
        }
    }

    // Average each 2×2 block (premultiplied) for smooth edges.
    let mut out = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0f32; 4];
            for sy in 0..ss {
                for sx in 0..ss {
                    let c = color[((y * ss + sy) * w + x * ss + sx) as usize];
                    let a = c[3] / 255.0;
                    sum[0] += c[0] * a;
                    sum[1] += c[1] * a;
                    sum[2] += c[2] * a;
                    sum[3] += c[3];
                }
            }
            let n = (ss * ss) as f32;
            let alpha = sum[3] / n;
            let un = if alpha > 0.0 { 255.0 / alpha } else { 0.0 };
            out.put_pixel(
                x,
                y,
                Rgba([
                    (sum[0] / n * un).min(255.0) as u8,
                    (sum[1] / n * un).min(255.0) as u8,
                    (sum[2] / n * un).min(255.0) as u8,
                    alpha as u8,
                ]),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {

    #[test]
    fn face_puts_the_hat_over_the_face() {
        let mut skin = RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 0, 0]));
        // Face pixel (0,0) is red; the hat pixel above it is half-transparent blue.
        skin.put_pixel(8, 8, image::Rgba([255, 0, 0, 255]));
        skin.put_pixel(40, 8, image::Rgba([0, 0, 255, 128]));
        // Another face pixel with no hat on it stays as it is.
        skin.put_pixel(9, 8, image::Rgba([0, 255, 0, 255]));
        let face = super::face(&skin, 16);
        assert_eq!(face.dimensions(), (16, 16));
        let blended = face.get_pixel(0, 0);
        assert!(blended[0] > 100 && blended[2] > 100, "{blended:?}");
        assert_eq!(*face.get_pixel(2, 0), image::Rgba([0, 255, 0, 255]));
        assert_eq!(
            face.get_pixel(1, 1),
            face.get_pixel(0, 0),
            "pixels stay crisp"
        );
    }

    use super::*;

    /// A skin with each body part's front a different color.
    fn test_skin() -> RgbaImage {
        let mut skin = RgbaImage::from_pixel(64, 64, Rgba([90, 90, 90, 255]));
        let fill = |skin: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, c: [u8; 4]| {
            for yy in y..y + h {
                for xx in x..x + w {
                    skin.put_pixel(xx, yy, Rgba(c));
                }
            }
        };
        // Outer layers transparent.
        fill(&mut skin, 32, 0, 32, 16, [0, 0, 0, 0]);
        fill(&mut skin, 0, 32, 64, 16, [0, 0, 0, 0]);
        fill(&mut skin, 0, 48, 16, 16, [0, 0, 0, 0]);
        fill(&mut skin, 48, 48, 16, 16, [0, 0, 0, 0]);
        fill(&mut skin, 8, 8, 8, 8, [255, 0, 0, 255]); // face: red
        fill(&mut skin, 20, 20, 8, 12, [0, 255, 0, 255]); // chest: green
        fill(&mut skin, 4, 20, 4, 12, [0, 0, 255, 255]); // right leg front: blue
        skin
    }

    fn dominant(img: &RgbaImage, x: u32, y: u32) -> usize {
        let Rgba([r, g, b, _]) = *img.get_pixel(x, y);
        [r, g, b]
            .iter()
            .enumerate()
            .max_by_key(|(_, v)| **v)
            .map(|(i, _)| i)
            .unwrap()
    }

    #[test]
    fn front_view_puts_parts_where_they_belong() {
        let skin = test_skin();
        let img = render(
            &skin,
            None,
            Model::Classic,
            View {
                yaw: 0.0,
                pitch: 0.0,
            },
            220,
            340,
        );
        // Model height spans ~86% of the image; centre column.
        let fit = 340.0 * 0.86 / 34.0;
        let y_of = |model_y: f32| (170.0 - (model_y - 16.0) * fit) as u32;
        assert_eq!(dominant(&img, 110, y_of(28.0)), 0, "face should be red");
        assert_eq!(dominant(&img, 110, y_of(18.0)), 1, "chest should be green");
        // Player's right leg is on the viewer's left.
        let x_left_leg = (110.0 - 2.0 * fit) as u32;
        assert_eq!(
            dominant(&img, x_left_leg, y_of(6.0)),
            2,
            "right leg should be blue"
        );
        // Corners stay transparent.
        assert_eq!(img.get_pixel(2, 2)[3], 0);
    }

    #[test]
    fn back_view_hides_the_face() {
        let skin = test_skin();
        let img = render(
            &skin,
            None,
            Model::Classic,
            View {
                yaw: std::f32::consts::PI,
                pitch: 0.0,
            },
            220,
            340,
        );
        let fit = 340.0 * 0.86 / 34.0;
        let p = img.get_pixel(110, (170.0 - 12.0 * fit) as u32);
        assert!(p[0] < 200, "back of the head isn't red: {p:?}");
        assert_eq!(p[3], 255);
    }

    #[test]
    fn old_skins_get_left_limbs() {
        let mut old = RgbaImage::from_pixel(64, 32, Rgba([0, 0, 0, 0]));
        old.put_pixel(4, 20, Rgba([1, 2, 3, 255])); // right leg front, top-left texel
        let skin = normalize(&old).unwrap();
        assert_eq!(skin.dimensions(), (64, 64));
        // Mirrored into the left leg front (20..24, 52..64) at the right edge.
        assert_eq!(*skin.get_pixel(23, 52), Rgba([1, 2, 3, 255]));
        assert!(normalize(&RgbaImage::new(32, 32)).is_none());
    }

    #[test]
    fn tells_slim_arms_apart() {
        let mut slim = RgbaImage::from_pixel(64, 64, Rgba([9, 9, 9, 255]));
        for y in 20..32 {
            slim.put_pixel(54, y, Rgba([0, 0, 0, 0]));
            slim.put_pixel(55, y, Rgba([0, 0, 0, 0]));
        }
        assert!(looks_slim(&slim));
        assert!(!looks_slim(&RgbaImage::from_pixel(
            64,
            64,
            Rgba([9, 9, 9, 255])
        )));
    }

    #[test]
    fn a_cape_shows_from_behind() {
        let skin = test_skin();
        let cape = RgbaImage::from_pixel(64, 32, Rgba([250, 240, 0, 255]));
        let img = render(
            &skin,
            Some(&cape),
            Model::Slim,
            View {
                yaw: std::f32::consts::PI,
                pitch: 0.0,
            },
            220,
            340,
        );
        let fit = 340.0 * 0.86 / 34.0;
        let Rgba([r, g, b, _]) = *img.get_pixel(110, (170.0 - 0.0 * fit) as u32);
        assert!(
            r > 150 && g > 150 && b < 60,
            "cape colour behind the body: {r} {g} {b}"
        );
    }
}
