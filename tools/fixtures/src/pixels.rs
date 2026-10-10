//! Procedural pictures. Each kind has a recognizable look (a photo-like
//! scene, a phone UI, a captioned meme, a page of text) without any real
//! content. The same scene seed renders the same scene, so a camera photo
//! and a phone photo "of the same moment" look alike.

use image::{imageops, Rgb, RgbImage};

/// Deterministic xorshift generator.
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % ((hi - lo).max(1) as u64)) as i64
    }
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.next() as usize % items.len()]
    }
}

fn hsv(h: f64, s: f64, v: f64) -> Rgb<u8> {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i as i32 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    Rgb([(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8])
}

fn fill_rect(img: &mut RgbImage, x: i64, y: i64, w: i64, h: i64, c: Rgb<u8>) {
    let (iw, ih) = (img.width() as i64, img.height() as i64);
    for yy in y.max(0)..(y + h).min(ih) {
        for xx in x.max(0)..(x + w).min(iw) {
            img.put_pixel(xx as u32, yy as u32, c);
        }
    }
}

fn fill_circle(img: &mut RgbImage, cx: i64, cy: i64, r: i64, c: Rgb<u8>) {
    let (iw, ih) = (img.width() as i64, img.height() as i64);
    for yy in (cy - r).max(0)..(cy + r).min(ih) {
        for xx in (cx - r).max(0)..(cx + r).min(iw) {
            if (xx - cx).pow(2) + (yy - cy).pow(2) <= r * r {
                img.put_pixel(xx as u32, yy as u32, c);
            }
        }
    }
}

fn noise(img: &mut RgbImage, rng: &mut Rng, amount: f64) {
    for p in img.pixels_mut() {
        let n = ((rng.f() - 0.5) * amount) as i32;
        for c in p.0.iter_mut() {
            *c = (*c as i32 + n).clamp(0, 255) as u8;
        }
    }
}

/// A landscape-like scene: sky gradient, horizon, sun, hills, objects.
/// `variant` shifts framing and color slightly (another camera, same scene).
pub fn scene(w: u32, h: u32, seed: u64, variant: u64) -> RgbImage {
    let mut r = Rng::new(seed);
    let hue = r.f();
    let horizon = 0.45 + r.f() * 0.2;
    let ground_hue = 0.2 + r.f() * 0.15;
    let sun = (r.f(), r.f() * horizon * 0.8, 0.04 + r.f() * 0.05);
    let objects: Vec<(f64, f64, f64, f64, f64)> = (0..(4 + r.range(0, 8))).map(|_| (r.f(), horizon + r.f() * 0.3, 0.03 + r.f() * 0.12, 0.05 + r.f() * 0.3, r.f())).collect();
    let mut v = Rng::new(seed ^ variant.wrapping_mul(7919));
    let shift = if variant == 0 { 0.0 } else { (v.f() - 0.5) * 0.08 };
    let warm = if variant == 0 { 0.0 } else { (v.f() - 0.5) * 0.06 };
    let mut img = RgbImage::new(w, h);
    for y in 0..h {
        let fy = y as f64 / h as f64 + shift;
        let c = if fy < horizon {
            hsv(0.55 + hue * 0.1 + warm, 0.5 - fy * 0.4, 0.95 - fy * 0.3)
        } else {
            let hill = ((fy - horizon) * 3.0).min(1.0);
            hsv(ground_hue + warm, 0.55, 0.65 - hill * 0.35)
        };
        for x in 0..w {
            img.put_pixel(x, y, c);
        }
    }
    let (fw, fh) = (w as f64, h as f64);
    fill_circle(&mut img, ((sun.0 + shift) * fw) as i64, (sun.1 * fh) as i64, (sun.2 * fw.min(fh)) as i64, Rgb([255, 236, 180]));
    for (ox, oy, ow, oh, oc) in objects {
        let c = hsv(oc + warm, 0.6, 0.35 + oc * 0.4);
        fill_rect(&mut img, ((ox + shift) * fw) as i64, ((oy - oh) * fh) as i64, (ow * fw) as i64, (oh * fh) as i64, c);
    }
    noise(&mut img, &mut v, 18.0);
    img
}

/// A phone screen: status bar, list rows of "text", a button bar.
pub fn screenshot(w: u32, h: u32, seed: u64, dark: bool) -> RgbImage {
    let mut r = Rng::new(seed);
    let (bg, fg) = if dark { (Rgb([18, 18, 20]), Rgb([230, 230, 235])) } else { (Rgb([248, 248, 250]), Rgb([30, 30, 34])) };
    let mut img = RgbImage::from_pixel(w, h, bg);
    let accent = hsv(r.f(), 0.7, 0.85);
    let u = (w / 40).max(4) as i64;
    fill_rect(&mut img, 0, 0, w as i64, u * 3, if dark { Rgb([0, 0, 0]) } else { Rgb([255, 255, 255]) });
    fill_rect(&mut img, u, u, u * 4, u, fg);
    fill_rect(&mut img, w as i64 - u * 6, u, u * 5, u, fg);
    let mut y = u * 6;
    while y < h as i64 - u * 10 {
        let line_w = (w as f64 * (0.3 + r.f() * 0.55)) as i64;
        if r.f() < 0.15 {
            fill_rect(&mut img, u * 2, y, w as i64 - u * 4, u * 8, hsv(r.f(), 0.4, 0.8));
            y += u * 10;
            continue;
        }
        fill_circle(&mut img, u * 3, y + u, u, accent);
        fill_rect(&mut img, u * 6, y, line_w, u, fg);
        fill_rect(&mut img, u * 6, y + u * 2, line_w / 2, u / 2 + 1, Rgb([140, 140, 145]));
        y += u * 5;
    }
    fill_rect(&mut img, 0, h as i64 - u * 7, w as i64, u * 7, if dark { Rgb([28, 28, 30]) } else { Rgb([240, 240, 242]) });
    for i in 0..5 {
        fill_circle(&mut img, (w as i64 / 5) * i + w as i64 / 10, h as i64 - u * 4, u, if i == 0 { accent } else { fg });
    }
    img
}

/// A game-like screen: green map, paths, circles (for Teach examples).
pub fn game_screen(w: u32, h: u32, seed: u64) -> RgbImage {
    let mut r = Rng::new(seed);
    let mut img = RgbImage::from_pixel(w, h, Rgb([120, 200, 120]));
    for _ in 0..12 {
        let y = r.range(0, h as i64);
        fill_rect(&mut img, 0, y, w as i64, (w / 60) as i64, Rgb([235, 235, 210]));
    }
    for _ in 0..6 {
        fill_circle(&mut img, r.range(0, w as i64), r.range(0, h as i64), (w / 14) as i64, Rgb([60, 140, 230]));
    }
    fill_circle(&mut img, w as i64 / 2, h as i64 * 2 / 3, (w / 10) as i64, Rgb([230, 60, 60]));
    fill_rect(&mut img, 0, h as i64 - (h / 8) as i64, w as i64, (h / 8) as i64, Rgb([250, 250, 250]));
    img
}

/// Bars of dark "words" on a light band.
fn caption(img: &mut RgbImage, r: &mut Rng, y: i64, band: i64, band_color: Rgb<u8>, text: Rgb<u8>) {
    let w = img.width() as i64;
    fill_rect(img, 0, y, w, band, band_color);
    let word_h = band / 3;
    let mut x = w / 12;
    let line_y = y + band / 2 - word_h / 2;
    while x < w - w / 12 {
        let ww = (w as f64 * (0.04 + r.f() * 0.1)) as i64;
        fill_rect(img, x, line_y, ww.min(w - w / 12 - x), word_h, text);
        x += ww + word_h / 2;
    }
}

/// A captioned meme: a picture with a white caption band, or impact-style
/// bars over it, or a four-panel comic.
pub fn meme(w: u32, h: u32, seed: u64) -> RgbImage {
    let mut r = Rng::new(seed);
    match r.range(0, 3) {
        0 => {
            let band = (h as f64 * 0.22) as u32;
            let pic = scene(w, h - band, seed, 0);
            let mut img = RgbImage::from_pixel(w, h, Rgb([255, 255, 255]));
            imageops::overlay(&mut img, &pic, 0, band as i64);
            caption(&mut img, &mut r, 0, band as i64, Rgb([255, 255, 255]), Rgb([10, 10, 10]));
            img
        }
        1 => {
            let mut img = scene(w, h, seed, 0);
            let band = (h / 7) as i64;
            caption(&mut img, &mut r, h as i64 / 40, band, Rgb([20, 20, 20]), Rgb([250, 250, 250]));
            caption(&mut img, &mut r, h as i64 - band - h as i64 / 40, band, Rgb([20, 20, 20]), Rgb([250, 250, 250]));
            img
        }
        _ => {
            let mut img = RgbImage::from_pixel(w, h, Rgb([255, 255, 255]));
            let (pw, ph) = (w / 2 - 6, h / 2 - 6);
            for i in 0..4u32 {
                let mut p = RgbImage::from_pixel(pw, ph, hsv(r.f(), 0.25, 0.97));
                fill_circle(&mut p, (pw / 2) as i64, (ph * 2 / 3) as i64, (pw / 5) as i64, hsv(r.f(), 0.5, 0.8));
                caption(&mut p, &mut r, 4, (ph / 5) as i64, Rgb([255, 255, 255]), Rgb([0, 0, 0]));
                imageops::overlay(&mut img, &p, (4 + (i % 2) * (pw + 4)) as i64, (4 + (i / 2) * (ph + 4)) as i64);
            }
            img
        }
    }
}

/// A saved social post: a photo with a profile header and an action row.
pub fn social(w: u32, h: u32, seed: u64) -> RgbImage {
    let mut img = scene(w, h, seed ^ 0x5050, 0);
    let mut r = Rng::new(seed);
    let u = (w / 30) as i64;
    fill_rect(&mut img, 0, 0, w as i64, u * 3, Rgb([255, 255, 255]));
    fill_circle(&mut img, u * 2, u * 3 / 2, u, hsv(r.f(), 0.8, 0.8));
    fill_rect(&mut img, u * 4, u, u * 8, u, Rgb([20, 20, 20]));
    fill_rect(&mut img, 0, h as i64 - u * 3, w as i64, u * 3, Rgb([255, 255, 255]));
    for i in 0..3 {
        fill_circle(&mut img, u * 2 + i * u * 3, h as i64 - u * 3 / 2, u * 2 / 3, Rgb([30, 30, 30]));
    }
    img
}

/// Smooth gradients with soft circles, no texture.
pub fn wallpaper(w: u32, h: u32, seed: u64) -> RgbImage {
    let mut r = Rng::new(seed);
    let (h1, h2) = (r.f(), r.f());
    let mut img = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let t = (x as f64 / w as f64 * 0.4 + y as f64 / h as f64 * 0.6).min(1.0);
            img.put_pixel(x, y, hsv(h1 + (h2 - h1) * t, 0.65, 0.35 + 0.5 * (1.0 - t)));
        }
    }
    for _ in 0..8 {
        let c = hsv(r.f(), 0.4, 0.95);
        fill_circle(&mut img, r.range(0, w as i64), r.range(0, h as i64), r.range((w / 30) as i64, (w / 8) as i64), c);
    }
    imageops::blur(&img, (w as f32 / 200.0).max(2.0))
}

/// A page or receipt on a table: white paper with rows of gray "text".
pub fn document(w: u32, h: u32, seed: u64, receipt: bool) -> RgbImage {
    let mut r = Rng::new(seed);
    let mut img = RgbImage::from_pixel(w, h, Rgb([110, 90, 70]));
    let (px, py) = (w as i64 / if receipt { 4 } else { 12 }, h as i64 / 16);
    let (pw, ph) = (w as i64 - px * 2, h as i64 - py * 2);
    fill_rect(&mut img, px, py, pw, ph, Rgb([245, 243, 236]));
    let line = (ph / 50).max(3);
    let mut y = py + line * 3;
    while y < py + ph - line * 3 {
        let lw = if receipt { (pw as f64 * (0.3 + r.f() * 0.6)) as i64 } else { (pw as f64 * (0.6 + r.f() * 0.3)) as i64 };
        fill_rect(&mut img, px + line * 2, y, lw - line * 4, line, Rgb([60, 60, 66]));
        if receipt {
            fill_rect(&mut img, px + pw - line * 8, y, line * 6, line, Rgb([60, 60, 66]));
        }
        y += line * if r.f() < 0.1 { 5 } else { 2 };
    }
    noise(&mut img, &mut r, 10.0);
    img
}

pub enum JunkKind {
    Black,
    White,
    Blurred,
    Pocket,
}

pub fn junk(w: u32, h: u32, seed: u64, kind: JunkKind) -> RgbImage {
    let mut r = Rng::new(seed);
    match kind {
        JunkKind::Black => {
            let mut img = RgbImage::from_pixel(w, h, Rgb([4, 4, 5]));
            noise(&mut img, &mut r, 6.0);
            img
        }
        JunkKind::White => {
            let mut img = RgbImage::from_pixel(w, h, Rgb([252, 252, 252]));
            noise(&mut img, &mut r, 4.0);
            img
        }
        JunkKind::Blurred => imageops::blur(&scene(w, h, seed, 0), w as f32 / 25.0),
        JunkKind::Pocket => {
            let mut img = imageops::blur(&scene(w, h, seed, 0), w as f32 / 30.0);
            for p in img.pixels_mut() {
                for c in p.0.iter_mut() {
                    *c /= 6;
                }
            }
            img
        }
    }
}

pub fn jpeg(img: &RgbImage, quality: u8) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality).encode_image(img).expect("jpeg encode");
    out
}

pub fn png(img: &RgbImage) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).expect("png encode");
    out.into_inner()
}
