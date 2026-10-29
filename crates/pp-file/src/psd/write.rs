//! `.psd` の書き出し（「複製を保存」）。CLIP STUDIO や Photoshop で続きを描けるよう、紙をそれぞれラスターレイヤーにする。
//!
//! - 紙は下から順に、名前・表示・不透明度をそのままに通常のレイヤーにする。めくった紙は非表示のレイヤーになる。
//! - 用紙の白地（油彩では下地）も、一番下のレイヤーとして書く。
//! - トレーシングペーパーのもやは、その紙のすぐ下に半透明の白いレイヤーを挟んで再現する。
//! - 紙の素材・パレット・キャンバスモードは残らない。
//!
//! 8 ビット RGB で、画素は PackBits（RLE）で圧縮する。読み込みと同じく Adobe の公開仕様に従う。

use std::io::{self, Write};
use std::path::Path;

use pp_core::tile::split_coord;
use pp_core::{Document, Layer, Rect, TILE_SIZE};

use crate::png::unpremultiply;
use crate::scaled::OutputSize;

use super::{CHANNEL_ALPHA, MAX_SIDE, MODE_RGB, RESOURCE_RESOLUTION};

/// RLE（PackBits）の印。
const COMPRESSION_RLE: u16 = 1;
const COMPRESSION_RAW: u16 = 0;
/// レイヤーの「非表示」の印。
const FLAG_HIDDEN: u8 = 0x02;
/// 書くチャンネル（透明度、赤、緑、青）。
const CHANNELS: [i16; 4] = [CHANNEL_ALPHA, 0, 1, 2];

/// `path` に保存する。いったん別名で書き終えてから置き換える。
pub fn save_to_path(path: &Path, document: &Document) -> io::Result<()> {
    crate::write_replacing(path, |out| save(out, document))
}

/// `document` を `.psd` として `out` に書く。
pub fn save(out: &mut impl Write, document: &Document) -> io::Result<()> {
    let (width, height) = (document.width, document.height);
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("PSD は一辺 {MAX_SIDE} ピクセルまでです")));
    }
    out.write_all(b"8BPS")?;
    out.write_all(&1u16.to_be_bytes())?;
    out.write_all(&[0; 6])?;
    out.write_all(&3u16.to_be_bytes())?;
    out.write_all(&height.to_be_bytes())?;
    out.write_all(&width.to_be_bytes())?;
    out.write_all(&8u16.to_be_bytes())?;
    out.write_all(&MODE_RGB.to_be_bytes())?;
    // 色のデータ（インデックスカラー用）はなし。画像リソースは解像度だけ
    out.write_all(&0u32.to_be_bytes())?;
    let resolution = resolution_resource(document.dpi);
    out.write_all(&(resolution.len() as u32).to_be_bytes())?;
    out.write_all(&resolution)?;

    let layer_info = layer_info(document);
    // レイヤー情報の長さ・中身、全体のマスク（なし）
    out.write_all(&(4 + layer_info.len() as u32 + 4).to_be_bytes())?;
    out.write_all(&(layer_info.len() as u32).to_be_bytes())?;
    out.write_all(&layer_info)?;
    out.write_all(&0u32.to_be_bytes())?;

    write_merged(out, document)
}

/// 解像度の画像リソース（ResolutionInfo）。横・縦とも `dpi`、表示の単位はインチ。
fn resolution_resource(dpi: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity(16);
    for _ in 0..2 {
        // 解像度（16.16 の固定小数点）、解像度の単位（1 = 1 インチあたり）、大きさの単位（1 = インチ）
        data.extend_from_slice(&(dpi << 16).to_be_bytes());
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&1u16.to_be_bytes());
    }
    let mut block = Vec::with_capacity(28);
    block.extend_from_slice(b"8BIM");
    block.extend_from_slice(&RESOURCE_RESOLUTION.to_be_bytes());
    // 名前は空（長さ 0 と、2 バイトに揃える 0）
    block.extend_from_slice(&[0, 0]);
    block.extend_from_slice(&(data.len() as u32).to_be_bytes());
    block.extend_from_slice(&data);
    block
}

/// PSD のレイヤー 1 枚になるもの。
struct PsdLayer<'a> {
    name: String,
    visible: bool,
    opacity: u8,
    pixels: Source<'a>,
}

enum Source<'a> {
    Sheet(&'a Layer),
    /// 作品全体を覆う 1 色（トレーシングペーパーのもや、straight の RGBA）。
    Fill([u8; 4]),
}

/// 書くレイヤーを下から順に並べる。
fn psd_layers(document: &Document) -> Vec<PsdLayer<'_>> {
    let mut layers = Vec::new();
    for layer in &document.layers {
        if let Some(veil) = layer.material.veil() {
            layers.push(PsdLayer {
                name: format!("{}のもや", layer.name),
                visible: layer.visible,
                opacity: 255,
                pixels: Source::Fill(unpremultiply(veil)),
            });
        }
        layers.push(PsdLayer {
            name: layer.name.clone(),
            visible: layer.visible,
            opacity: layer.opacity,
            pixels: Source::Sheet(layer),
        });
    }
    layers
}

/// レイヤー情報：レイヤーの数、各レイヤーの記録、各レイヤーのチャンネルの画素の順。長さは偶数に揃える。
fn layer_info(document: &Document) -> Vec<u8> {
    let bounds = document.bounds();
    let layers = psd_layers(document);
    let channels: Vec<(Rect, [Vec<u8>; 4])> = layers.iter().map(|layer| layer.channels(bounds)).collect();

    let mut info = Vec::new();
    info.extend_from_slice(&(layers.len() as u16).to_be_bytes());
    for (layer, (rect, data)) in layers.iter().zip(&channels) {
        layer.write_record(&mut info, *rect, data);
    }
    for (_, data) in &channels {
        for channel in data {
            info.extend_from_slice(channel);
        }
    }
    if info.len() % 2 == 1 {
        info.push(0);
    }
    info
}

impl PsdLayer<'_> {
    /// 書く範囲と、チャンネルごとの圧縮した画素（`CHANNELS` の順）。
    fn channels(&self, bounds: Rect) -> (Rect, [Vec<u8>; 4]) {
        let rect = match self.pixels {
            Source::Sheet(layer) => drawn_rect(layer, bounds),
            Source::Fill(_) => bounds,
        };
        if rect.is_empty() {
            return (rect, std::array::from_fn(|_| COMPRESSION_RAW.to_be_bytes().to_vec()));
        }
        let mut planes: [RlePlane; 4] = std::array::from_fn(|_| RlePlane::new(rect.height() as usize));
        let mut row = vec![[0u8; 4]; rect.width() as usize];
        for y in rect.y0..rect.y1 {
            match self.pixels {
                Source::Sheet(layer) => layer_row(layer, y, rect, &mut row),
                Source::Fill(color) => row.fill(color),
            }
            // 透明度、赤、緑、青の順
            for (plane, c) in planes.iter_mut().zip([3, 0, 1, 2]) {
                plane.push_row(row.iter().map(|p| p[c]));
            }
        }
        (rect, planes.map(RlePlane::into_channel))
    }

    fn write_record(&self, out: &mut Vec<u8>, rect: Rect, channels: &[Vec<u8>; 4]) {
        for v in [rect.y0, rect.x0, rect.y1, rect.x1] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out.extend_from_slice(&(CHANNELS.len() as u16).to_be_bytes());
        for (id, data) in CHANNELS.iter().zip(channels) {
            out.extend_from_slice(&id.to_be_bytes());
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        }
        out.extend_from_slice(b"8BIMnorm");
        out.push(self.opacity);
        // クリッピングなし
        out.push(0);
        out.push(if self.visible { 0 } else { FLAG_HIDDEN });
        out.push(0);

        let mut extra = Vec::new();
        // マスクと合成の条件はなし
        extra.extend_from_slice(&0u32.to_be_bytes());
        extra.extend_from_slice(&0u32.to_be_bytes());
        // 古い形式の名前（1 バイト文字だけ）。4 バイト単位に揃える。読むソフトは Unicode の名前を使う
        let ascii: Vec<u8> = self.name.bytes().filter(|b| b.is_ascii_graphic() || *b == b' ').take(255).collect();
        extra.push(ascii.len() as u8);
        extra.extend_from_slice(&ascii);
        extra.resize(extra.len() + (4 - (ascii.len() + 1) % 4) % 4, 0);
        // Unicode の名前（`luni`）。長さを 4 バイト単位に揃えるため、必要なら最後に 0 を足す
        let mut units: Vec<u16> = self.name.encode_utf16().collect();
        let mut luni = Vec::new();
        luni.extend_from_slice(&(units.len() as u32).to_be_bytes());
        if units.len() % 2 == 1 {
            units.push(0);
        }
        units.iter().for_each(|u| luni.extend_from_slice(&u.to_be_bytes()));
        extra.extend_from_slice(b"8BIMluni");
        extra.extend_from_slice(&(luni.len() as u32).to_be_bytes());
        extra.extend_from_slice(&luni);

        out.extend_from_slice(&(extra.len() as u32).to_be_bytes());
        out.extend_from_slice(&extra);
    }
}

/// 描いたタイルを囲む範囲（作品の中だけ）。
fn drawn_rect(layer: &Layer, bounds: Rect) -> Rect {
    let ts = TILE_SIZE as i32;
    layer
        .tiles()
        .filter(|(_, tile)| !tile.is_transparent())
        .map(|((tx, ty), _)| Rect::new(tx * ts, ty * ts, (tx + 1) * ts, (ty + 1) * ts))
        .fold(Rect::EMPTY, |acc, r| acc.union(&r))
        .intersect(&bounds)
}

/// `layer` の行 `y` のうち `rect` の横の範囲を、straight の RGBA で `out` に入れる。
fn layer_row(layer: &Layer, y: i32, rect: Rect, out: &mut [[u8; 4]]) {
    let ts = TILE_SIZE as i32;
    let (ty, oy) = split_coord(y);
    let mut x = rect.x0;
    while x < rect.x1 {
        let (tx, ox) = split_coord(x);
        let end = ((tx + 1) * ts).min(rect.x1);
        let dst = &mut out[(x - rect.x0) as usize..(end - rect.x0) as usize];
        match layer.tile(tx, ty) {
            Some(tile) => {
                let src = &tile.row(oy)[ox as usize..ox as usize + dst.len()];
                dst.iter_mut().zip(src).for_each(|(d, s)| *d = unpremultiply(*s));
            }
            None => dst.fill([0; 4]),
        }
        x = end;
    }
}

/// 統合した画像（RGB）。開いたソフトがレイヤーを読めないときや、一覧の見本に使われる。
fn write_merged(out: &mut impl Write, document: &Document) -> io::Result<()> {
    let height = document.height as usize;
    let mut planes: [RlePlane; 3] = std::array::from_fn(|_| RlePlane::new(height));
    OutputSize::of(document).for_each_row(document, |row| {
        for (c, plane) in planes.iter_mut().enumerate() {
            // 白い紙の上に置いた色にする（紙モードも油彩も一番下は不透明なので、ふつうはそのまま）
            plane.push_row(row.iter().map(|p| [p.r, p.g, p.b][c] + (255 - p.a)));
        }
    });
    out.write_all(&COMPRESSION_RLE.to_be_bytes())?;
    // 全チャンネル分の行の長さの表を先にまとめて書き、そのあとに各チャンネルの中身を書く
    for plane in &planes {
        for &len in &plane.row_lengths {
            out.write_all(&len.to_be_bytes())?;
        }
    }
    for plane in &planes {
        out.write_all(&plane.body)?;
    }
    Ok(())
}

/// 1 チャンネル分の PackBits。行ごとの長さと、つなげた中身。
struct RlePlane {
    row_lengths: Vec<u16>,
    body: Vec<u8>,
    /// 行を圧縮する前に置いておく場所（使い回す）。
    scratch: Vec<u8>,
}

impl RlePlane {
    fn new(rows: usize) -> Self {
        RlePlane { row_lengths: Vec::with_capacity(rows), body: Vec::new(), scratch: Vec::new() }
    }

    fn push_row(&mut self, values: impl Iterator<Item = u8>) {
        self.scratch.clear();
        self.scratch.extend(values);
        let start = self.body.len();
        pack_bits(&self.scratch, &mut self.body);
        // 一辺 30000 ピクセルまでなので、最悪でも 1 行 65535 バイトに収まる
        self.row_lengths.push((self.body.len() - start) as u16);
    }

    /// レイヤーのチャンネルの形（圧縮の印、行の長さの表、中身）にする。
    fn into_channel(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 + self.row_lengths.len() * 2 + self.body.len());
        out.extend_from_slice(&COMPRESSION_RLE.to_be_bytes());
        self.row_lengths.iter().for_each(|len| out.extend_from_slice(&len.to_be_bytes()));
        out.extend_from_slice(&self.body);
        out
    }
}

/// PackBits：同じ値が 3 つ以上続く所は「繰り返し」、ほかは「そのまま」として、どちらも 128 個ずつまで書く。
fn pack_bits(row: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < row.len() {
        let run = row[i..].iter().take(128).take_while(|&&v| v == row[i]).count();
        if run >= 3 {
            out.push((1 - run as i32) as i8 as u8);
            out.push(row[i]);
            i += run;
            continue;
        }
        // 次に 3 つ以上の繰り返しが始まる所まで（最大 128）をそのまま書く
        let mut end = i;
        while end < row.len() && end - i < 128 {
            if end + 2 < row.len() && row[end] == row[end + 1] && row[end] == row[end + 2] {
                break;
            }
            end += 1;
        }
        out.push((end - i - 1) as u8);
        out.extend_from_slice(&row[i..end]);
        i = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::psd::{load, unpack_rle};
    use pp_core::{CanvasMode, Rgba8, SheetMaterial};

    fn saved(document: &Document) -> Vec<u8> {
        let mut bytes = Vec::new();
        save(&mut bytes, document).unwrap();
        bytes
    }

    fn composite(document: &Document) -> Vec<Rgba8> {
        let mut out = vec![Rgba8::default(); (document.width * document.height) as usize];
        document.composite(document.bounds(), &mut out);
        out
    }

    #[test]
    fn pack_bits_round_trips() {
        let rows: [Vec<u8>; 5] = [
            vec![],
            vec![7],
            vec![1, 2, 2, 3, 3, 3, 3, 4, 5, 5],
            (0..=255).chain(std::iter::repeat_n(9, 300)).collect(),
            (0..1000).map(|i| (i / 3 % 2) as u8).collect(),
        ];
        for row in rows {
            let mut packed = (0u16).to_be_bytes().to_vec();
            pack_bits(&row, &mut packed);
            let len = packed.len() - 2;
            packed[..2].copy_from_slice(&(len as u16).to_be_bytes());
            assert_eq!(unpack_rle(&packed, 1, row.len(), false).unwrap(), row);
        }
    }

    #[test]
    fn sheets_become_layers_that_open_again() {
        let mut document = Document::new(300, 200, CanvasMode::Paper);
        document.dpi = 600;
        document.layers[1].set_pixel(10, 20, Rgba8 { r: 0, g: 0, b: 255, a: 255 });
        document.add_sheet(SheetMaterial::TracingPaper);
        document.active_layer_mut().set_pixel(299, 199, Rgba8 { r: 128, g: 0, b: 0, a: 128 });
        document.add_sheet(SheetMaterial::ClearFilm);
        document.active_layer_mut().set_pixel(150, 100, Rgba8 { r: 255, g: 0, b: 0, a: 255 });
        document.flip_active_sheet();

        let imported = load(&saved(&document)).unwrap();
        assert_eq!(imported.document.dpi, 600, "解像度も戻る");
        let names: Vec<&str> = imported.document.sheets().map(|(_, l)| l.name.as_str()).collect();
        let expected_veil = format!("{}のもや", document.layers[2].name);
        // 一番下（白地）は、読み込むときに用紙の地として名前が付け直される
        assert_eq!(
            names[1..],
            ["用紙", expected_veil.as_str(), document.layers[2].name.as_str(), document.layers[3].name.as_str()]
        );
        let flipped = imported.document.sheets().last().unwrap().1;
        assert!(!flipped.visible, "めくった紙は非表示のレイヤー");
        assert_eq!(flipped.pixel(150, 100), Rgba8 { r: 255, g: 0, b: 0, a: 255 });

        // もやを挟んだので、見た目は元と同じになる
        for (a, b) in composite(&document).iter().zip(composite(&imported.document)) {
            for (x, y) in [(a.r, b.r), (a.g, b.g), (a.b, b.b), (a.a, b.a)] {
                assert!(x.abs_diff(y) <= 2, "{a:?} と {b:?}");
            }
        }
    }

    #[test]
    fn writes_the_merged_image() {
        let mut document = Document::new(5, 3, CanvasMode::OilCanvas);
        document.layers[1].set_pixel(4, 2, Rgba8 { r: 10, g: 20, b: 30, a: 255 });
        let bytes = saved(&document);
        // 統合画像は最後にある。3 チャンネル × 3 行の長さの表のあとに中身
        let resources = u32::from_be_bytes(bytes[30..34].try_into().unwrap()) as usize;
        let at = 34 + resources;
        let layer_section = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let merged = &bytes[at + 4 + layer_section..];
        assert_eq!(merged[..2], COMPRESSION_RLE.to_be_bytes());
        let lengths: Vec<usize> =
            merged[2..20].as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes([c[0], c[1]]) as usize).collect();
        let mut body = &merged[20..];
        let mut planes = Vec::new();
        for c in 0..3 {
            let rows = &lengths[c * 3..c * 3 + 3];
            let mut table: Vec<u8> = rows.iter().flat_map(|&n| (n as u16).to_be_bytes()).collect();
            let total: usize = rows.iter().sum();
            table.extend_from_slice(&body[..total]);
            body = &body[total..];
            planes.push(unpack_rle(&table, 3, 5, false).unwrap());
        }
        assert!(body.is_empty());
        assert_eq!([planes[0][14], planes[1][14], planes[2][14]], [10, 20, 30]);
        assert_eq!([planes[0][0], planes[1][0], planes[2][0]], [244, 240, 230], "下地の色");
    }

    #[test]
    fn empty_sheet_has_no_pixels() {
        let document = Document::new(64, 64, CanvasMode::Paper);
        let layers = psd_layers(&document);
        let (rect, channels) = layers[1].channels(document.bounds());
        assert!(rect.is_empty());
        assert!(channels.iter().all(|c| c == &COMPRESSION_RAW.to_be_bytes()));
    }
}
