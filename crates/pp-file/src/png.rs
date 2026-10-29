//! 見た目どおりに合成した絵を PNG 画像として書き出す。めくった紙は含めない。
//!
//! PNG は zlib 圧縮した画素に CRC を付けたチャンクを並べるだけなので、`.plim` と同じ圧縮処理で自前で書く。

use std::io::{self, Write};
use std::path::Path;

use miniz_oxide::deflate::compress_to_vec_zlib;
use pp_core::{Document, Rgba8};

use crate::scaled::OutputSize;

pub const EXTENSION: &str = "png";

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
const COMPRESSION_LEVEL: u8 = 6;

/// `path` に書き出す。いったん別名で書き終えてから置き換える。
pub fn export_to_path(path: &Path, document: &Document, size: OutputSize, dpi: u32) -> io::Result<()> {
    crate::write_replacing(path, |out| export(out, document, size, dpi))
}

/// 8 ビット RGBA（透過あり）の PNG を、`size` の大きさ・解像度 `dpi` で `out` に書く。
pub fn export(out: &mut impl Write, document: &Document, size: OutputSize, dpi: u32) -> io::Result<()> {
    let (width, height) = (size.width(), size.height());
    out.write_all(&SIGNATURE)?;

    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    // ビット深度 8、RGBA、圧縮・フィルタ・インターレースは標準
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    write_chunk(out, b"IHDR", &header)?;

    // 解像度（pHYs）は 1 メートルあたりの画素数で書く
    let per_meter = (dpi as f64 / 0.0254).round() as u32;
    let mut physical = Vec::with_capacity(9);
    physical.extend_from_slice(&per_meter.to_be_bytes());
    physical.extend_from_slice(&per_meter.to_be_bytes());
    physical.push(1);
    write_chunk(out, b"pHYs", &physical)?;

    // 各行の先頭にフィルタの種類（0 = なし）を置いた画素列
    let row_bytes = width as usize * 4 + 1;
    let mut raw = Vec::with_capacity(row_bytes * height as usize);
    size.for_each_row(document, |row| {
        raw.push(0);
        raw.extend(row.iter().flat_map(|p| unpremultiply(*p)));
    });
    write_chunk(out, b"IDAT", &compress_to_vec_zlib(&raw, COMPRESSION_LEVEL))?;
    write_chunk(out, b"IEND", &[])
}

/// premultiplied の画素を、PNG が使う直線の RGBA に戻す。
pub(crate) fn unpremultiply(p: Rgba8) -> [u8; 4] {
    match p.a {
        0 => [0; 4],
        255 => [p.r, p.g, p.b, 255],
        a => {
            let undo = |c: u8| ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
            [undo(p.r), undo(p.g), undo(p.b), a]
        }
    }
}

fn write_chunk(out: &mut impl Write, kind: &[u8; 4], data: &[u8]) -> io::Result<()> {
    out.write_all(&(data.len() as u32).to_be_bytes())?;
    out.write_all(kind)?;
    out.write_all(data)?;
    let crc = Crc32::new().update(kind).update(data).finish();
    out.write_all(&crc.to_be_bytes())
}

/// PNG のチャンクに付ける CRC-32。
struct Crc32 {
    value: u32,
}

impl Crc32 {
    fn new() -> Self {
        Crc32 { value: 0xffff_ffff }
    }

    fn update(mut self, bytes: &[u8]) -> Self {
        for &byte in bytes {
            self.value ^= byte as u32;
            for _ in 0..8 {
                let mask = (self.value & 1).wrapping_neg();
                self.value = (self.value >> 1) ^ (0xedb8_8320 & mask);
            }
        }
        self
    }

    fn finish(self) -> u32 {
        !self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use miniz_oxide::inflate::decompress_to_vec_zlib;
    use pp_core::{CanvasMode, SheetMaterial};

    /// 書き出した PNG のチャンクを `(種類, 中身)` で並べる。CRC も確かめる。
    fn chunks(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
        assert_eq!(png[..8], SIGNATURE);
        let mut found = Vec::new();
        let mut rest = &png[8..];
        while !rest.is_empty() {
            let len = u32::from_be_bytes(rest[..4].try_into().unwrap()) as usize;
            let kind: [u8; 4] = rest[4..8].try_into().unwrap();
            let data = rest[8..8 + len].to_vec();
            let crc = u32::from_be_bytes(rest[8 + len..12 + len].try_into().unwrap());
            assert_eq!(crc, Crc32::new().update(&kind).update(&data).finish());
            found.push((kind, data));
            rest = &rest[12 + len..];
        }
        found
    }

    #[test]
    fn crc_matches_known_value() {
        assert_eq!(Crc32::new().update(b"123456789").finish(), 0xcbf4_3926);
    }

    #[test]
    fn exports_what_is_visible() {
        let mut document = Document::new(300, 260, CanvasMode::Paper);
        document.layers[1].set_pixel(299, 259, Rgba8 { r: 0, g: 0, b: 255, a: 255 });
        document.add_sheet(SheetMaterial::ClearFilm);
        document.active_layer_mut().set_pixel(0, 0, Rgba8 { r: 255, g: 0, b: 0, a: 255 });
        document.flip_active_sheet();

        let mut png = Vec::new();
        export(&mut png, &document, OutputSize::of(&document), 350).unwrap();
        let found = chunks(&png);
        let kinds: Vec<_> = found.iter().map(|(kind, _)| kind).collect();
        assert_eq!(kinds, [b"IHDR", b"pHYs", b"IDAT", b"IEND"]);
        assert_eq!(found[0].1[..8], [0, 0, 1, 44, 0, 0, 1, 4]);
        // 350dpi は 1 メートルあたり 13780 画素
        assert_eq!(found[1].1, [0, 0, 0x35, 0xd4, 0, 0, 0x35, 0xd4, 1]);

        let raw = decompress_to_vec_zlib(&found[2].1).unwrap();
        let row_bytes = 300 * 4 + 1;
        assert_eq!(raw.len(), row_bytes * 260);
        let pixel = |x: usize, y: usize| &raw[y * row_bytes + 1 + x * 4..][..4];
        assert_eq!(pixel(0, 0), [255, 255, 255, 255], "めくった紙は書き出さない");
        assert_eq!(pixel(299, 259), [0, 0, 255, 255]);
    }

    #[test]
    fn exports_at_the_chosen_size() {
        let document = Document::new(300, 200, CanvasMode::Paper);
        let mut png = Vec::new();
        export(&mut png, &document, OutputSize::new(525, 350), 350).unwrap();
        let found = chunks(&png);
        assert_eq!(found[0].1[..8], [0, 0, 2, 13, 0, 0, 1, 94]);
        let raw = decompress_to_vec_zlib(&found[2].1).unwrap();
        assert_eq!(raw.len(), (525 * 4 + 1) * 350);
    }

    #[test]
    fn unpremultiply_restores_straight_color() {
        assert_eq!(unpremultiply(Rgba8 { r: 64, g: 0, b: 128, a: 128 }), [128, 0, 255, 128]);
        assert_eq!(unpremultiply(Rgba8::default()), [0; 4]);
    }
}
