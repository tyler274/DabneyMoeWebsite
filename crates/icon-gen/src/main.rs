//! Generates the 1024x1024 source app icon: a dark rounded-square tile with a
//! cyan->blue gradient and a white ">_" developer glyph. Feed the output to
//! `cargo tauri icon` to emit every size/format Tauri needs.
//!
//! Usage: `cargo run -p icon-gen [-- OUTPUT_PATH]`
//! (default OUTPUT_PATH: `src-tauri/icons/source.png`)

use image::{Rgb, RgbImage};

const SIZE: u32 = 1024;

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Shortest distance from point `(px, py)` to segment `a`-`b`.
fn dist_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    if dx == 0.0 && dy == 0.0 {
        return (px - ax).hypot(py - ay);
    }
    let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    (px - (ax + t * dx)).hypot(py - (ay + t * dy))
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "src-tauri/icons/source.png".to_string());

    let bg = [2u8, 6, 23]; // slate-950
    let g_top = [34.0, 211.0, 238.0]; // cyan-400
    let g_bot = [37.0, 99.0, 235.0]; // blue-600
    let glyph = [255u8, 255, 255];

    let (lo, hi) = (96.0, SIZE as f32 - 96.0);
    let radius = 180.0;
    let c = SIZE as f32 / 2.0;
    let thickness = 46.0;
    // A ">" chevron (two segments) plus an underscore bar.
    let chev = [
        (c - 150.0, c - 170.0, c + 70.0, c),
        (c + 70.0, c, c - 150.0, c + 170.0),
    ];
    let bar = (c - 150.0, c + 240.0, c + 190.0, c + 240.0);

    let mut img = RgbImage::from_pixel(SIZE, SIZE, Rgb(bg));

    for y in 0..SIZE {
        for x in 0..SIZE {
            let (fx, fy) = (x as f32, y as f32);
            if fx < lo || fx > hi || fy < lo || fy > hi {
                continue;
            }
            // Rounded corners.
            if (fx < lo + radius || fx > hi - radius) && (fy < lo + radius || fy > hi - radius) {
                let corx = if fx < lo + radius {
                    lo + radius
                } else {
                    hi - radius
                };
                let cory = if fy < lo + radius {
                    lo + radius
                } else {
                    hi - radius
                };
                if (fx - corx).hypot(fy - cory) > radius {
                    continue;
                }
            }

            let t = (fy - lo) / (hi - lo);
            let mut px = [
                lerp(g_top[0], g_bot[0], t) as u8,
                lerp(g_top[1], g_bot[1], t) as u8,
                lerp(g_top[2], g_bot[2], t) as u8,
            ];

            let d = dist_to_segment(fx, fy, chev[0].0, chev[0].1, chev[0].2, chev[0].3)
                .min(dist_to_segment(
                    fx, fy, chev[1].0, chev[1].1, chev[1].2, chev[1].3,
                ))
                .min(dist_to_segment(fx, fy, bar.0, bar.1, bar.2, bar.3));
            if d <= thickness / 2.0 {
                px = glyph;
            }

            img.put_pixel(x, y, Rgb(px));
        }
    }

    img.save(&out).expect("failed to write icon PNG");
    println!("wrote {out} ({SIZE}x{SIZE})");
}
