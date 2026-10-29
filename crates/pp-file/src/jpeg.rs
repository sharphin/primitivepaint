//! 見た目どおりに合成した絵を JPEG 画像として書き出す。JPEG は透明を持てないので、透けている所は白い紙の上に置いた色にする。
//!
//! 基本の JPEG（ベースライン）を自前で書く。ハフマン符号は規格（ITU-T T.81）付録 K の表をそのまま使い、
//! 量子化の表は付録 K の表を品質に合わせて伸び縮みさせる（IJG の libjpeg と同じ計算なので、品質の数字の感覚も同じ）。
//! 線画の色の縁がにじまないよう、色差は間引かない（4:4:4）。出力は 8 行ずつ作るので、大きい絵でもメモリは少なくて済む。

use std::io::{self, Write};
use std::path::Path;

use pp_core::{Document, Rgba8};

use crate::scaled::OutputSize;

pub const EXTENSIONS: [&str; 2] = ["jpg", "jpeg"];

/// 品質の範囲と、はじめに選ばれている値。
pub const MIN_QUALITY: u8 = 1;
pub const MAX_QUALITY: u8 = 100;
pub const DEFAULT_QUALITY: u8 = 90;

/// JPEG の 1 ブロックは 8×8 画素。
const BLOCK: usize = 8;

/// ブロック内の位置を、低い周波数から順に並べた順番（ジグザグ）。`ZIGZAG[i]` は i 番目に書く係数の位置。
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21,
    28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54,
    47, 55, 62, 63,
];

/// 量子化の基準の表（付録 K.1 輝度、K.2 色差）。並びはブロック内の位置の順。
const LUMA_QUANT: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56, 14, 17, 22, 29, 51,
    87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113, 92, 49, 64, 78, 87, 103, 121, 120, 101,
    72, 92, 95, 98, 112, 100, 103, 99,
];
const CHROMA_QUANT: [u16; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99, 47, 66, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99,
];

/// ハフマン符号の表（付録 K.3〜K.6）。長さごとの符号の数と、短い符号から順の値。
const LUMA_DC_COUNTS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const LUMA_DC_VALUES: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const CHROMA_DC_COUNTS: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
const CHROMA_DC_VALUES: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const LUMA_AC_COUNTS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
const LUMA_AC_VALUES: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14,
    0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09,
    0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a,
    0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65,
    0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88,
    0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9,
    0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca,
    0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea,
    0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
];
const CHROMA_AC_COUNTS: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
const CHROMA_AC_VALUES: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71, 0x13, 0x22, 0x32,
    0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0, 0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16,
    0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39,
    0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64,
    0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86,
    0x87, 0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
    0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8,
    0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9,
    0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
];

/// 係数の続きがすべて 0 の印（EOB）と、0 が 16 個続く印（ZRL）。
const END_OF_BLOCK: u8 = 0x00;
const SIXTEEN_ZEROS: u8 = 0xf0;

/// `path` に、品質 `quality`（1〜100）・解像度 `dpi` で書き出す。いったん別名で書き終えてから置き換える。
pub fn export_to_path(path: &Path, document: &Document, size: OutputSize, quality: u8, dpi: u32) -> io::Result<()> {
    crate::write_replacing(path, |out| export(out, document, size, quality, dpi))
}

/// `size` の大きさの JPEG を `out` に書く。
pub fn export(out: &mut impl Write, document: &Document, size: OutputSize, quality: u8, dpi: u32) -> io::Result<()> {
    let (width, height) = (size.width(), size.height());
    if width > u16::MAX as u32 || height > u16::MAX as u32 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "JPEG は一辺 65535 ピクセルまでです"));
    }
    let mut encoder = Encoder::new(quality);
    encoder.write_headers(out, width as u16, height as u16, dpi.min(u16::MAX as u32) as u16)?;

    // 8 行たまるごとに 1 列分のブロックを符号にする
    let row_len = width as usize;
    let mut strip: Vec<[f32; 3]> = Vec::with_capacity(row_len * BLOCK);
    let mut bits = BitWriter::new(out);
    size.for_each_row(document, |row| {
        strip.extend(row.iter().map(|p| to_ycbcr(on_white(*p))));
        if strip.len() == row_len * BLOCK {
            encoder.encode_strip(&mut bits, &strip, row_len);
            strip.clear();
        }
    });
    if !strip.is_empty() {
        // 最後の半端な行は、一番下の行を繰り返して埋める
        let last = strip[strip.len() - row_len..].to_vec();
        while strip.len() < row_len * BLOCK {
            strip.extend_from_slice(&last);
        }
        encoder.encode_strip(&mut bits, &strip, row_len);
    }
    let out = bits.finish()?;
    out.write_all(&[0xff, 0xd9])
}

/// premultiplied の画素を白い紙の上に置いた色にする。
fn on_white(p: Rgba8) -> [u8; 3] {
    let white = 255 - p.a;
    [p.r + white, p.g + white, p.b + white]
}

/// RGB を JPEG（JFIF）の YCbCr にし、0 を中心にする（各値から 128 を引く）。
fn to_ycbcr([r, g, b]: [u8; 3]) -> [f32; 3] {
    let (r, g, b) = (r as f32, g as f32, b as f32);
    [
        0.299 * r + 0.587 * g + 0.114 * b - 128.0,
        -0.168_736 * r - 0.331_264 * g + 0.5 * b,
        0.5 * r - 0.418_688 * g - 0.081_312 * b,
    ]
}

/// 品質に合わせた量子化の表（IJG と同じ計算）。
fn scaled_quant(base: &[u16; 64], quality: u8) -> [u16; 64] {
    let q = quality.clamp(MIN_QUALITY, MAX_QUALITY) as u32;
    let percent = if q < 50 { 5000 / q } else { 200 - q * 2 };
    base.map(|v| ((v as u32 * percent + 50) / 100).clamp(1, 255) as u16)
}

/// 1 つの色の成分（Y・Cb・Cr）の符号の作り方。
struct Component {
    quant: [u16; 64],
    dc: HuffmanTable,
    ac: HuffmanTable,
    /// 直前のブロックの直流成分。直流成分は前との差を書く。
    previous_dc: i32,
}

struct Encoder {
    components: [Component; 3],
    /// DCT の基底。`basis[u][x]` は周波数 u の、位置 x での重み。
    basis: [[f32; BLOCK]; BLOCK],
}

impl Encoder {
    fn new(quality: u8) -> Self {
        let luma = || Component {
            quant: scaled_quant(&LUMA_QUANT, quality),
            dc: HuffmanTable::new(&LUMA_DC_COUNTS, &LUMA_DC_VALUES),
            ac: HuffmanTable::new(&LUMA_AC_COUNTS, &LUMA_AC_VALUES),
            previous_dc: 0,
        };
        let chroma = || Component {
            quant: scaled_quant(&CHROMA_QUANT, quality),
            dc: HuffmanTable::new(&CHROMA_DC_COUNTS, &CHROMA_DC_VALUES),
            ac: HuffmanTable::new(&CHROMA_AC_COUNTS, &CHROMA_AC_VALUES),
            previous_dc: 0,
        };
        let mut basis = [[0.0; BLOCK]; BLOCK];
        for (u, line) in basis.iter_mut().enumerate() {
            let c = if u == 0 { std::f32::consts::FRAC_1_SQRT_2 } else { 1.0 };
            for (x, weight) in line.iter_mut().enumerate() {
                *weight = c / 2.0 * ((2 * x + 1) as f32 * u as f32 * std::f32::consts::PI / 16.0).cos();
            }
        }
        Encoder { components: [luma(), chroma(), chroma()], basis }
    }

    /// 画像の大きさや表などを書く（JFIF の印、量子化の表、画像の大きさ、ハフマン符号の表、符号の始まり）。
    fn write_headers(&self, out: &mut impl Write, width: u16, height: u16, dpi: u16) -> io::Result<()> {
        out.write_all(&[0xff, 0xd8])?;
        // JFIF 1.01、解像度の単位は 1 インチあたり（1）で、横・縦とも `dpi`。見本の縮小画像はなし
        let [hi, lo] = dpi.to_be_bytes();
        write_segment(out, 0xe0, &[b'J', b'F', b'I', b'F', 0, 1, 1, 1, hi, lo, hi, lo, 0, 0])?;
        for (id, table) in [(0u8, &self.components[0].quant), (1, &self.components[1].quant)] {
            let mut data = vec![id];
            data.extend(ZIGZAG.iter().map(|&i| table[i] as u8));
            write_segment(out, 0xdb, &data)?;
        }
        let mut frame = vec![8];
        frame.extend_from_slice(&height.to_be_bytes());
        frame.extend_from_slice(&width.to_be_bytes());
        // 成分は 3 つ。間引きなし（縦横とも 1）、量子化の表は Y が 0 番、色差が 1 番
        frame.extend_from_slice(&[3, 1, 0x11, 0, 2, 0x11, 1, 3, 0x11, 1]);
        write_segment(out, 0xc0, &frame)?;
        let tables: [(u8, &[u8; 16], &[u8]); 4] = [
            (0x00, &LUMA_DC_COUNTS, &LUMA_DC_VALUES),
            (0x10, &LUMA_AC_COUNTS, &LUMA_AC_VALUES),
            (0x01, &CHROMA_DC_COUNTS, &CHROMA_DC_VALUES),
            (0x11, &CHROMA_AC_COUNTS, &CHROMA_AC_VALUES),
        ];
        for (class_id, counts, values) in tables {
            let mut data = vec![class_id];
            data.extend_from_slice(counts);
            data.extend_from_slice(values);
            write_segment(out, 0xc4, &data)?;
        }
        // 3 つの成分をまとめて符号にする。Y は 0 番、色差は 1 番のハフマン符号を使う
        write_segment(out, 0xda, &[3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0])
    }

    /// 8 行分の画素（YCbCr）を、左のブロックから順に符号にする。
    fn encode_strip<W: Write>(&mut self, bits: &mut BitWriter<W>, strip: &[[f32; 3]], row_len: usize) {
        for x0 in (0..row_len).step_by(BLOCK) {
            for (c, component) in self.components.iter_mut().enumerate() {
                let mut block = [0.0f32; 64];
                for (y, line) in block.as_chunks_mut::<BLOCK>().0.iter_mut().enumerate() {
                    for (x, value) in line.iter_mut().enumerate() {
                        // 右端の半端なブロックは、一番右の画素を繰り返して埋める
                        *value = strip[y * row_len + (x0 + x).min(row_len - 1)][c];
                    }
                }
                let coefficients = quantized_dct(&self.basis, &block, &component.quant);
                component.encode_block(bits, &coefficients);
            }
        }
    }
}

/// 2 次元 DCT（基底 `basis`）をかけて量子化した係数（ブロック内の位置の順）。
fn quantized_dct(basis: &[[f32; BLOCK]; BLOCK], block: &[f32; 64], quant: &[u16; 64]) -> [i32; 64] {
    // 行ごと、列ごとの順に 1 次元の DCT をかける
    let mut rows = [0.0f32; 64];
    for y in 0..BLOCK {
        for u in 0..BLOCK {
            rows[y * BLOCK + u] = (0..BLOCK).map(|x| basis[u][x] * block[y * BLOCK + x]).sum();
        }
    }
    let mut out = [0i32; 64];
    for v in 0..BLOCK {
        for u in 0..BLOCK {
            let sum: f32 = (0..BLOCK).map(|y| basis[v][y] * rows[y * BLOCK + u]).sum();
            out[v * BLOCK + u] = (sum / quant[v * BLOCK + u] as f32).round() as i32;
        }
    }
    out
}

impl Component {
    fn encode_block<W: Write>(&mut self, bits: &mut BitWriter<W>, coefficients: &[i32; 64]) {
        let dc = coefficients[0];
        let diff = dc - self.previous_dc;
        self.previous_dc = dc;
        let (size, extra) = magnitude(diff);
        self.dc.write(bits, size);
        bits.write(extra, size);

        let mut zeros = 0;
        for &i in &ZIGZAG[1..] {
            let value = coefficients[i];
            if value == 0 {
                zeros += 1;
                continue;
            }
            while zeros >= 16 {
                self.ac.write(bits, SIXTEEN_ZEROS);
                zeros -= 16;
            }
            let (size, extra) = magnitude(value);
            self.ac.write(bits, (zeros << 4) | size);
            bits.write(extra, size);
            zeros = 0;
        }
        if zeros > 0 {
            self.ac.write(bits, END_OF_BLOCK);
        }
    }
}

/// 値を書くのに要るビット数と、そのビット列（負の数は 1 を引いた値の下位ビット）。
fn magnitude(value: i32) -> (u8, u32) {
    let size = (32 - value.unsigned_abs().leading_zeros()) as u8;
    let extra = if value < 0 { (value - 1) as u32 & ((1u32 << size) - 1) } else { value as u32 };
    (size, extra)
}

/// 値ごとのハフマン符号（符号, ビット数）。
struct HuffmanTable {
    codes: [(u16, u8); 256],
}

impl HuffmanTable {
    /// 長さごとの符号の数と値の並びから、規格の決まり（付録 C）どおりに符号を割り当てる。
    fn new(counts: &[u8; 16], values: &[u8]) -> Self {
        let mut codes = [(0, 0); 256];
        let mut code = 0u16;
        let mut next = values.iter();
        for (i, &count) in counts.iter().enumerate() {
            for _ in 0..count {
                let value = *next.next().expect("表の値の数は符号の数と同じ");
                codes[value as usize] = (code, i as u8 + 1);
                code += 1;
            }
            code <<= 1;
        }
        HuffmanTable { codes }
    }

    fn write<W: Write>(&self, bits: &mut BitWriter<W>, value: u8) {
        let (code, len) = self.codes[value as usize];
        debug_assert!(len > 0, "表にない値 {value:#x}");
        bits.write(code as u32, len);
    }
}

/// 符号をビット単位で書く。0xff のバイトの後には、印と区別するため 0 を挟む。
/// 書き込みの失敗は覚えておき、最後の `finish` で返す（符号を作る途中では止めない）。
struct BitWriter<'a, W: Write> {
    out: &'a mut W,
    buffer: Vec<u8>,
    bits: u32,
    count: u8,
    error: Option<io::Error>,
}

impl<'a, W: Write> BitWriter<'a, W> {
    /// ためておく量。これを超えたら書き出す。
    const FLUSH_AT: usize = 64 * 1024;

    fn new(out: &'a mut W) -> Self {
        BitWriter { out, buffer: Vec::with_capacity(Self::FLUSH_AT + 16), bits: 0, count: 0, error: None }
    }

    /// `value` の下位 `len` ビットを、上の桁から書く。
    fn write(&mut self, value: u32, len: u8) {
        if len == 0 {
            return;
        }
        self.bits = (self.bits << len) | (value & ((1 << len) - 1));
        self.count += len;
        while self.count >= 8 {
            self.count -= 8;
            let byte = (self.bits >> self.count) as u8;
            self.buffer.push(byte);
            if byte == 0xff {
                self.buffer.push(0);
            }
        }
        self.bits &= (1 << self.count) - 1;
        if self.buffer.len() >= Self::FLUSH_AT {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.error.is_none()
            && let Err(err) = self.out.write_all(&self.buffer)
        {
            self.error = Some(err);
        }
        self.buffer.clear();
    }

    /// 半端なビットを 1 で埋めて書き終える。
    fn finish(mut self) -> io::Result<&'a mut W> {
        if self.count > 0 {
            let pad = 8 - self.count;
            self.write((1 << pad) - 1, pad);
        }
        self.flush();
        match self.error {
            Some(err) => Err(err),
            None => Ok(self.out),
        }
    }
}

/// 印（0xff と種類）、長さ、中身の順に書く。
fn write_segment(out: &mut impl Write, marker: u8, data: &[u8]) -> io::Result<()> {
    out.write_all(&[0xff, marker])?;
    out.write_all(&(data.len() as u16 + 2).to_be_bytes())?;
    out.write_all(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pp_core::CanvasMode;

    /// 印の種類と中身を順に並べる。符号の部分（SOS の後）は飛ばす。
    fn segments(jpeg: &[u8]) -> Vec<(u8, Vec<u8>)> {
        assert_eq!(jpeg[..2], [0xff, 0xd8]);
        assert_eq!(jpeg[jpeg.len() - 2..], [0xff, 0xd9]);
        let mut found = Vec::new();
        let mut i = 2;
        loop {
            assert_eq!(jpeg[i], 0xff);
            let marker = jpeg[i + 1];
            let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
            found.push((marker, jpeg[i + 4..i + 2 + len].to_vec()));
            i += 2 + len;
            if marker == 0xda {
                return found;
            }
        }
    }

    #[test]
    fn writes_the_size_and_tables() {
        let document = Document::new(21, 13, CanvasMode::Paper);
        let mut jpeg = Vec::new();
        export(&mut jpeg, &document, OutputSize::of(&document), DEFAULT_QUALITY, 600).unwrap();
        let found = segments(&jpeg);
        let markers: Vec<u8> = found.iter().map(|(m, _)| *m).collect();
        assert_eq!(markers, [0xe0, 0xdb, 0xdb, 0xc0, 0xc4, 0xc4, 0xc4, 0xc4, 0xda]);
        // 単位は 1 インチあたり、600dpi
        assert_eq!(found[0].1[7..12], [1, 2, 88, 2, 88]);
        assert_eq!(found[3].1[1..5], [0, 13, 0, 21]);
    }

    #[test]
    fn quality_scales_the_tables() {
        assert_eq!(scaled_quant(&LUMA_QUANT, 50), LUMA_QUANT);
        assert!(scaled_quant(&LUMA_QUANT, 100).iter().all(|&q| q == 1));
        assert_eq!(scaled_quant(&LUMA_QUANT, 1)[0], 255);
    }

    #[test]
    fn huffman_codes_follow_the_standard() {
        // 付録 K.3：輝度の直流の 0 は 00、1 は 010、11 は 111111110
        let table = HuffmanTable::new(&LUMA_DC_COUNTS, &LUMA_DC_VALUES);
        assert_eq!(table.codes[0], (0b00, 2));
        assert_eq!(table.codes[1], (0b010, 3));
        assert_eq!(table.codes[11], (0b1_1111_1110, 9));
        // 付録 K.5：輝度の交流の EOB は 1010、ZRL は 11111111001
        let table = HuffmanTable::new(&LUMA_AC_COUNTS, &LUMA_AC_VALUES);
        assert_eq!(table.codes[0x00], (0b1010, 4));
        assert_eq!(table.codes[0xf0], (0b111_1111_1001, 11));
    }

    #[test]
    fn magnitude_of_negative_values() {
        assert_eq!(magnitude(0), (0, 0));
        assert_eq!(magnitude(5), (3, 0b101));
        assert_eq!(magnitude(-5), (3, 0b010));
        assert_eq!(magnitude(-1), (1, 0));
    }

    #[test]
    fn flat_white_block_has_only_the_mean() {
        let encoder = Encoder::new(100);
        let white = [255.0 - 128.0; 64];
        let coefficients = quantized_dct(&encoder.basis, &white, &[1; 64]);
        assert_eq!(coefficients[0], 1016, "直流成分は平均の 8 倍");
        assert!(coefficients[1..].iter().all(|&c| c == 0));
    }

    #[test]
    fn transparent_is_white() {
        assert_eq!(on_white(Rgba8::default()), [255, 255, 255]);
        assert_eq!(on_white(Rgba8 { r: 0, g: 0, b: 128, a: 128 }), [127, 127, 255]);
    }

    #[test]
    fn stuffs_zero_after_ff() {
        let mut out = Vec::new();
        let mut bits = BitWriter::new(&mut out);
        bits.write(0xff, 8);
        bits.write(0b1, 1);
        bits.finish().unwrap();
        assert_eq!(out, [0xff, 0x00, 0xff, 0x00]);
    }
}
