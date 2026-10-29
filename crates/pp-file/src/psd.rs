//! Photoshop の形式（`.psd` と大きい作品用の `.psb`）の読み込みと、`.psd` の書き出し（`write`）。
//! CLIP STUDIO などほかのソフトとのやりとりに使う。
//!
//! 形式は Adobe の公開仕様（Photoshop File Formats Specification）のとおりに読む。
//! レイヤーは `layered` で primitivepaint の紙に変換する（不透明度・合成モード・マスクは焼き込む）。
//! 読めるのは RGB とグレースケールの 8・16 ビット。

use std::path::Path;

use miniz_oxide::inflate::decompress_to_vec_zlib_with_limit;
use pp_core::tile::split_coord;
use pp_core::{Document, Layer, Rect, Rgba8, TILE_SIZE};

use crate::FileError;
use crate::blend::Blend;
use crate::layered::{self, Content, Imported, LayerNode, Mask, NodeBlend};

mod write;

pub use write::{save, save_to_path};

pub const EXTENSIONS: [&str; 2] = ["psd", "psb"];
/// 「複製を保存」で付ける拡張子。作品の一辺は `.psd` の上限より小さいので、`.psb` は書かない。
pub const SAVE_EXTENSION: &str = "psd";

/// 読める作品の一辺の上限（ピクセル）。
const MAX_SIDE: u32 = 30000;

/// 色の持ち方。
const MODE_GRAYSCALE: u16 = 1;
const MODE_RGB: u16 = 3;

/// チャンネルの番号。
const CHANNEL_ALPHA: i16 = -1;
const CHANNEL_MASK: i16 = -2;

/// フォルダの印（`lsct` の種類）。
const SECTION_OPEN: u32 = 1;
const SECTION_CLOSED: u32 = 2;
const SECTION_END: u32 = 3;

/// 画像リソースのうち、解像度（ResolutionInfo）の番号。
const RESOURCE_RESOLUTION: u16 = 0x03ed;

/// 色調補正レイヤーなど、画素を持たずに下の見た目を変えるレイヤーの印。
const ADJUSTMENT_KEYS: [&[u8; 4]; 19] = [
    b"brit", b"levl", b"curv", b"expA", b"vibA", b"hue ", b"hue2", b"blnc", b"blwh", b"phfl", b"mixr", b"clrL",
    b"nvrt", b"post", b"thrs", b"grdm", b"selc", b"CgEd", b"lut ",
];

pub fn load_from_path(path: &Path) -> Result<Imported, FileError> {
    let bytes = std::fs::read(path).map_err(FileError::Io)?;
    load(&bytes)
}

pub fn load(bytes: &[u8]) -> Result<Imported, FileError> {
    let mut r = Reader::new(bytes);
    if r.take(4)? != b"8BPS" {
        return Err(FileError::NotOurFile);
    }
    let big = match r.u16()? {
        1 => false,
        2 => true,
        _ => return Err(FileError::Corrupt("知らない版の PSD")),
    };
    r.take(6)?;
    let channels = r.u16()?;
    let height = r.u32()?;
    let width = r.u32()?;
    let depth = r.u16()?;
    let mode = r.u16()?;
    if width == 0 || height == 0 {
        return Err(FileError::Corrupt("作品の大きさ"));
    }
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(FileError::Unsupported(format!("一辺 {MAX_SIDE} ピクセルより大きい作品は読めません")));
    }
    if depth != 8 && depth != 16 {
        return Err(FileError::Unsupported(format!(
            "{depth} ビットの PSD は読めません（8 ビットか 16 ビットにしてください）"
        )));
    }
    if mode != MODE_RGB && mode != MODE_GRAYSCALE {
        return Err(FileError::Unsupported("RGB かグレースケールの PSD だけ読めます（CMYK などは読めません）".into()));
    }
    let format = Format { big, depth, gray: mode == MODE_GRAYSCALE };
    let bounds = Rect::from_size(width, height);

    let color_data = r.u32()? as usize;
    r.take(color_data)?;
    let resources = r.u32()? as usize;
    let dpi = read_resolution(r.take(resources)?);

    let layer_and_mask = r.length(big)?;
    let mut section = Reader::new(r.take(layer_and_mask)?);
    let mut records = Vec::new();
    if !section.is_empty() {
        let info_len = section.length(big)?;
        let info = section.take(info_len)?;
        if !info.is_empty() {
            records = read_layer_info(&mut Reader::new(info), &format, bounds)?;
        }
        // 全体のマスクを飛ばすと、作品全体の追加情報が並ぶ。16 ビットではレイヤーはここ（Lr16）に入る
        if records.is_empty() && section.remaining() >= 4 {
            let global_mask = section.u32()? as usize;
            section.take(global_mask)?;
            while section.remaining() >= 12 {
                let block = read_extra_block(&mut section, big, true)?;
                if matches!(&block.key, b"Lr16" | b"Lr32" | b"Layr") {
                    records = read_layer_info(&mut Reader::new(&block.data), &format, bounds)?;
                    break;
                }
            }
        }
    }

    let nodes = if records.is_empty() {
        // レイヤーのない PSD は、統合した画像を 1 枚の紙にする
        let pixels = read_merged(&mut r, &format, channels, bounds)?;
        vec![LayerNode {
            name: "背景".into(),
            visible: true,
            opacity: 1.0,
            blend: NodeBlend::Mode(Blend::Normal),
            clip: false,
            mask: None,
            content: Content::Pixels(pixels),
        }]
    } else {
        build_tree(records)?
    };
    let mut imported = layered::convert(width, height, None, nodes).ok_or(FileError::Corrupt("紙の構成"))?;
    if let Some(dpi) = dpi {
        imported.document.dpi = dpi;
    }
    Ok(imported)
}

/// 画像リソースから解像度（dpi）を探す。なければ、または読めなければ `None`（作品はモードの既定の解像度になる）。
fn read_resolution(resources: &[u8]) -> Option<u32> {
    let mut r = Reader::new(resources);
    while r.remaining() >= 12 {
        if r.take(4).ok()? != b"8BIM" {
            return None;
        }
        let id = r.u16().ok()?;
        // 名前は 2 バイト単位に揃えたパスカル文字列
        let name_len = r.u8().ok()? as usize;
        r.take(name_len + (name_len + 1) % 2).ok()?;
        let len = r.u32().ok()? as usize;
        let data = r.take(len).ok()?;
        r.take((len % 2).min(r.remaining())).ok()?;
        if id == RESOURCE_RESOLUTION && data.len() >= 4 {
            // 横の解像度は 16.16 の固定小数点。単位の欄が cm でも、値はいつも 1 インチあたり
            let fixed = u32::from_be_bytes(data[..4].try_into().ok()?);
            let dpi = (fixed as f64 / 65536.0).round() as u32;
            return Some(dpi.clamp(Document::MIN_DPI, Document::MAX_DPI));
        }
    }
    None
}

/// 画素の持ち方。
struct Format {
    /// PSB（大きい作品用）か。長さの欄が 8 バイトになる。
    big: bool,
    depth: u16,
    gray: bool,
}

/// 1 レイヤー分の記録。
struct Record {
    node: LayerNode,
    /// フォルダの印（`lsct`）。フォルダでなければ `None`。
    section: Option<u32>,
}

/// レイヤー情報：レイヤーの記録が並び、その後に各レイヤーのチャンネルの画素が同じ順で並ぶ。
fn read_layer_info(r: &mut Reader, format: &Format, bounds: Rect) -> Result<Vec<Record>, FileError> {
    // 負の数は「統合画像の 1 つ目のアルファが透明度」の意味で、数は絶対値
    let count = (r.u16()? as i16).unsigned_abs() as usize;
    let mut heads = Vec::with_capacity(count);
    for _ in 0..count {
        heads.push(read_record_head(r, format.big)?);
    }
    let mut records = Vec::with_capacity(count);
    for head in heads {
        let mut channels: Vec<(i16, Rect, Vec<u8>)> = Vec::new();
        for &(id, len) in &head.channels {
            let rect = if id == CHANNEL_MASK { head.mask.as_ref().map_or(Rect::EMPTY, |m| m.rect) } else { head.rect };
            let data = r.take(len)?;
            // ベクターマスク（-3 以下）などは使わないので読み飛ばす
            if id >= CHANNEL_MASK {
                channels.push((id, rect, decode_channel(data, rect, format)?));
            }
        }
        records.push(head.into_record(channels, format, bounds));
    }
    Ok(records)
}

/// レイヤーの記録の、画素より前の部分。
struct RecordHead {
    rect: Rect,
    channels: Vec<(i16, usize)>,
    blend: NodeBlend,
    opacity: u8,
    clip: bool,
    visible: bool,
    mask: Option<MaskHead>,
    name: String,
    section: Option<u32>,
    /// 色調補正など、読めない種類のレイヤーなら、その説明。
    unsupported: Option<&'static str>,
}

struct MaskHead {
    rect: Rect,
    fill: u8,
    enabled: bool,
}

fn read_rect(r: &mut Reader) -> Result<Rect, FileError> {
    let top = r.i32()?;
    let left = r.i32()?;
    let bottom = r.i32()?;
    let right = r.i32()?;
    if bottom < top || right < left {
        return Err(FileError::Corrupt("レイヤーの範囲"));
    }
    Ok(Rect::new(left, top, right, bottom))
}

fn read_record_head(r: &mut Reader, big: bool) -> Result<RecordHead, FileError> {
    let rect = read_rect(r)?;
    let channel_count = r.u16()? as usize;
    let mut channels = Vec::with_capacity(channel_count);
    for _ in 0..channel_count {
        let id = r.u16()? as i16;
        let len = r.length(big)?;
        channels.push((id, len));
    }
    if r.take(4)? != b"8BIM" {
        return Err(FileError::Corrupt("レイヤーの合成モードの印"));
    }
    let key: [u8; 4] = r.take(4)?.try_into().expect("4 バイト");
    let opacity = r.u8()?;
    let clip = r.u8()? != 0;
    let flags = r.u8()?;
    r.u8()?;
    let extra_len = r.u32()? as usize;
    let mut extra = Reader::new(r.take(extra_len)?);

    let mask_len = extra.u32()? as usize;
    let mask = if mask_len >= 18 {
        let mut m = Reader::new(extra.take(mask_len)?);
        let rect = read_rect(&mut m)?;
        let fill = m.u8()?;
        let mask_flags = m.u8()?;
        // 2 ビット目が「マスクを使わない」
        Some(MaskHead { rect, fill, enabled: mask_flags & 0x02 == 0 })
    } else {
        extra.take(mask_len)?;
        None
    };
    let ranges = extra.u32()? as usize;
    extra.take(ranges)?;
    // 名前は 4 バイト単位に揃えたパスカル文字列（古い形式。Unicode の名前があればそちらを使う）
    let name_len = extra.u8()? as usize;
    let ansi_name = extra.take(name_len)?.to_vec();
    extra.take((4 - (name_len + 1) % 4) % 4)?;

    let mut name = String::from_utf8_lossy(&ansi_name).into_owned();
    let mut section = None;
    let mut blend = blend_from_key(&key);
    let mut unsupported = None;
    while extra.remaining() >= 12 {
        let block = read_extra_block(&mut extra, big, false)?;
        match &block.key {
            b"luni" => {
                let mut u = Reader::new(&block.data);
                let len = u.u32()? as usize;
                let units: Vec<u16> = (0..len).map(|_| u.u16()).collect::<Result<_, _>>()?;
                name = String::from_utf16_lossy(&units).trim_end_matches('\0').to_string();
            }
            b"lsct" | b"lsdk" => {
                let mut s = Reader::new(&block.data);
                section = Some(s.u32()?);
                if s.remaining() >= 8 && s.take(4)? == b"8BIM" {
                    let folder_key: [u8; 4] = s.take(4)?.try_into().expect("4 バイト");
                    blend = blend_from_key(&folder_key);
                }
            }
            b"SoCo" | b"GdFl" | b"PtFl" => unsupported = Some("塗りつぶしレイヤー"),
            key if ADJUSTMENT_KEYS.contains(&key) => unsupported = Some("色調補正レイヤー"),
            _ => {}
        }
    }
    Ok(RecordHead {
        rect,
        channels,
        blend,
        opacity,
        clip,
        // 2 ビット目が「非表示」
        visible: flags & 0x02 == 0,
        mask,
        name,
        section,
        unsupported,
    })
}

fn blend_from_key(key: &[u8; 4]) -> NodeBlend {
    if key == b"pass" {
        return NodeBlend::PassThrough;
    }
    match Blend::from_psd_key(key) {
        Some(blend) => NodeBlend::Mode(blend),
        None => NodeBlend::Unknown(String::from_utf8_lossy(key).trim().to_string()),
    }
}

impl RecordHead {
    /// 展開したチャンネル（番号, 範囲, 1 画素 1 バイト）から、紙に変換する元のレイヤーを作る。
    fn into_record(self, channels: Vec<(i16, Rect, Vec<u8>)>, format: &Format, bounds: Rect) -> Record {
        let find = |id: i16| channels.iter().find(|(c, _, _)| *c == id).map(|(_, _, d)| d.as_slice());
        let content = if self.section.is_some_and(|s| s != 0) {
            // フォルダの中身は build_tree で入れる
            Content::Folder(Vec::new())
        } else if let Some(what) = self.unsupported {
            Content::Unsupported(what.to_string())
        } else {
            let color = if format.gray { [find(0); 3] } else { [find(0), find(1), find(2)] };
            Content::Pixels(pixels_layer(self.rect, color, find(CHANNEL_ALPHA), bounds))
        };
        let mask = match (self.mask, channels.iter().find(|(c, _, _)| *c == CHANNEL_MASK)) {
            // 範囲が空で既定が 255 のマスクは何も隠さない（マスクのないレイヤーにこう書くソフトがある）
            (Some(head), Some((_, rect, data))) if head.enabled && !(rect.is_empty() && head.fill == 255) => {
                Some(Mask { layer: mask_layer(*rect, data, head.fill, bounds), fill: head.fill })
            }
            _ => None,
        };
        Record {
            node: LayerNode {
                name: self.name,
                visible: self.visible,
                opacity: self.opacity as f32 / 255.0,
                blend: self.blend,
                clip: self.clip,
                mask,
                content,
            },
            section: self.section,
        }
    }
}

/// 下から順の記録を、フォルダの入れ子にする。フォルダは「終わりの印」→ 中身 → 「フォルダ本体」の順に並ぶ。
fn build_tree(records: Vec<Record>) -> Result<Vec<LayerNode>, FileError> {
    let mut stack: Vec<Vec<LayerNode>> = vec![Vec::new()];
    for record in records {
        match record.section {
            Some(SECTION_END) => stack.push(Vec::new()),
            Some(SECTION_OPEN | SECTION_CLOSED) => {
                let children = stack.pop().ok_or(FileError::Corrupt("フォルダの対応"))?;
                let mut folder = record.node;
                folder.content = Content::Folder(children);
                stack.last_mut().ok_or(FileError::Corrupt("フォルダの対応"))?.push(folder);
            }
            _ => stack.last_mut().expect("いつも 1 つはある").push(record.node),
        }
    }
    // 終わりの印だけで本体のないフォルダは、中身をそのまま上に戻す
    while stack.len() > 1 {
        let orphan = stack.pop().expect("2 つ以上ある");
        stack.last_mut().expect("1 つは残る").extend(orphan);
    }
    Ok(stack.pop().expect("1 つは残る"))
}

/// 追加情報の 1 ブロック。
struct ExtraBlock {
    key: [u8; 4],
    data: Vec<u8>,
}

/// `8BIM` か `8B64` の印、4 文字の種類、長さ、中身。PSB の一部の種類は長さが 8 バイト。
fn read_extra_block(r: &mut Reader, big: bool, global: bool) -> Result<ExtraBlock, FileError> {
    let signature = r.take(4)?;
    if signature != b"8BIM" && signature != b"8B64" {
        return Err(FileError::Corrupt("追加情報の印"));
    }
    let key: [u8; 4] = r.take(4)?.try_into().expect("4 バイト");
    let long = big
        && matches!(
            &key,
            b"LMsk"
                | b"Lr16"
                | b"Lr32"
                | b"Layr"
                | b"Mt16"
                | b"Mt32"
                | b"Mtrn"
                | b"Alph"
                | b"FMsk"
                | b"lnk2"
                | b"FEid"
                | b"FXid"
                | b"PxSD"
        );
    let len = if long { r.u64()? as usize } else { r.u32()? as usize };
    let data = r.take(len)?.to_vec();
    // 作品全体の追加情報は 4 バイト単位に揃えてある
    if global {
        r.take(((4 - len % 4) % 4).min(r.remaining()))?;
    }
    Ok(ExtraBlock { key, data })
}

/// 1 チャンネル分を展開し、`rect` の大きさの 1 画素 1 バイトにする（16 ビットは上位 8 ビット）。
fn decode_channel(data: &[u8], rect: Rect, format: &Format) -> Result<Vec<u8>, FileError> {
    let (w, h) = (rect.width() as usize, rect.height() as usize);
    let bytes_per = (format.depth / 8) as usize;
    let row_bytes = w * bytes_per;
    if data.len() < 2 {
        return Ok(vec![0; w * h]);
    }
    let compression = u16::from_be_bytes([data[0], data[1]]);
    let body = &data[2..];
    let raw = match compression {
        0 => body.to_vec(),
        1 => unpack_rle(body, h, row_bytes, format.big)?,
        2 | 3 => {
            let mut raw = decompress_to_vec_zlib_with_limit(body, row_bytes * h)
                .map_err(|_| FileError::Corrupt("画素データを展開できない"))?;
            if compression == 3 {
                undo_prediction(&mut raw, row_bytes, bytes_per);
            }
            raw
        }
        _ => return Err(FileError::Corrupt("知らない圧縮")),
    };
    if raw.len() < row_bytes * h {
        return Err(FileError::Corrupt("画素データが短い"));
    }
    Ok(match bytes_per {
        1 => raw[..w * h].to_vec(),
        _ => raw.as_chunks::<2>().0.iter().take(w * h).map(|p| p[0]).collect(),
    })
}

/// PackBits（行ごとの長さの表のあとに、各行の圧縮データ）。
fn unpack_rle(body: &[u8], rows: usize, row_bytes: usize, big: bool) -> Result<Vec<u8>, FileError> {
    let mut r = Reader::new(body);
    let counts: Vec<usize> = (0..rows)
        .map(|_| if big { r.u32().map(|v| v as usize) } else { r.u16().map(|v| v as usize) })
        .collect::<Result<_, _>>()?;
    let mut out = Vec::with_capacity(rows * row_bytes);
    for count in counts {
        let line = r.take(count)?;
        let start = out.len();
        let mut i = 0;
        while i < line.len() && out.len() - start < row_bytes {
            let n = line[i] as i8;
            i += 1;
            if n >= 0 {
                let len = n as usize + 1;
                out.extend_from_slice(line.get(i..i + len).ok_or(FileError::Corrupt("圧縮データが短い"))?);
                i += len;
            } else if n != -128 {
                let value = *line.get(i).ok_or(FileError::Corrupt("圧縮データが短い"))?;
                out.extend(std::iter::repeat_n(value, (1 - n as isize) as usize));
                i += 1;
            }
        }
        out.resize(start + row_bytes, 0);
    }
    Ok(out)
}

/// ZIP（予測あり）は、各行で左の画素との差が入っている。
fn undo_prediction(raw: &mut [u8], row_bytes: usize, bytes_per: usize) {
    for row in raw.chunks_mut(row_bytes) {
        if bytes_per == 1 {
            for i in 1..row.len() {
                row[i] = row[i].wrapping_add(row[i - 1]);
            }
        } else {
            let mut prev = 0u16;
            for pair in row.as_chunks_mut::<2>().0 {
                let v = u16::from_be_bytes(*pair).wrapping_add(prev);
                *pair = v.to_be_bytes();
                prev = v;
            }
        }
    }
}

/// `rect` に置いた色とアルファのチャンネル（premultiplied ではない）から、作品の座標のレイヤーを作る。
fn pixels_layer(rect: Rect, color: [Option<&[u8]>; 3], alpha: Option<&[u8]>, bounds: Rect) -> Layer {
    let get = |c: Option<&[u8]>, i: usize, default: u8| c.and_then(|c| c.get(i).copied()).unwrap_or(default);
    write_rect(rect, bounds, None, |i| {
        let a = get(alpha, i, 255) as u32;
        let mul = |v: u8| ((v as u32 * a + 127) / 255) as u8;
        Rgba8 { r: mul(get(color[0], i, 0)), g: mul(get(color[1], i, 0)), b: mul(get(color[2], i, 0)), a: a as u8 }
    })
    .unwrap_or_else(|| Layer::new(""))
}

/// マスクの濃さを `a` に入れたレイヤー。範囲の外（同じタイルの中）は `fill`。
fn mask_layer(rect: Rect, data: &[u8], fill: u8, bounds: Rect) -> Layer {
    write_rect(rect, bounds, Some(fill), |i| Rgba8 { r: 0, g: 0, b: 0, a: data.get(i).copied().unwrap_or(fill) })
        .unwrap_or_else(|| Layer::new(""))
}

/// `rect` の画素を `pixel(行優先の番号)` で作ってレイヤーに書く。`fill` があれば、作るタイルをその濃さで埋めておく。
/// 作品にかからなければ `None`。
fn write_rect(rect: Rect, bounds: Rect, fill: Option<u8>, pixel: impl Fn(usize) -> Rgba8) -> Option<Layer> {
    let area = rect.intersect(&bounds);
    if area.is_empty() {
        return None;
    }
    let mut layer = Layer::new("");
    let ts = TILE_SIZE as i32;
    let w = rect.width() as usize;
    let (tx0, _) = split_coord(area.x0);
    let (ty0, _) = split_coord(area.y0);
    let (tx1, _) = split_coord(area.x1 - 1);
    let (ty1, _) = split_coord(area.y1 - 1);
    for ty in ty0..=ty1 {
        for tx in tx0..=tx1 {
            let tile_rect = Rect::new(tx * ts, ty * ts, (tx + 1) * ts, (ty + 1) * ts);
            if let Some(a) = fill {
                layer.fill(tile_rect, Rgba8 { r: 0, g: 0, b: 0, a });
            }
            let r = area.intersect(&tile_rect);
            let tile = layer.tile_mut_or_create(tx, ty);
            for y in r.y0..r.y1 {
                let row = tile.row_mut((y - ty * ts) as u32);
                let base = (y - rect.y0) as usize * w;
                for x in r.x0..r.x1 {
                    row[(x - tx * ts) as usize] = pixel(base + (x - rect.x0) as usize);
                }
            }
        }
    }
    Some(layer)
}

/// レイヤーのない PSD の、統合した画像。
fn read_merged(r: &mut Reader, format: &Format, channels: u16, bounds: Rect) -> Result<Layer, FileError> {
    let compression = r.u16()?;
    let (w, h) = (bounds.width() as usize, bounds.height() as usize);
    let row_bytes = w * (format.depth / 8) as usize;
    let count = channels as usize;
    let planes: Vec<Vec<u8>> = match compression {
        0 => (0..count).map(|_| Ok(r.take(row_bytes * h)?.to_vec())).collect::<Result<_, FileError>>()?,
        1 => {
            // 全チャンネル分の行の長さの表が先にまとめて並ぶ
            let mut counts = Vec::with_capacity(count * h);
            for _ in 0..count * h {
                counts.push(if format.big { r.u32()? as usize } else { r.u16()? as usize });
            }
            let mut planes = Vec::with_capacity(count);
            for c in 0..count {
                let rows = &counts[c * h..(c + 1) * h];
                let mut table = Vec::new();
                for &n in rows {
                    if format.big {
                        table.extend_from_slice(&(n as u32).to_be_bytes());
                    } else {
                        table.extend_from_slice(&(n as u16).to_be_bytes());
                    }
                }
                let total: usize = rows.iter().sum();
                table.extend_from_slice(r.take(total)?);
                planes.push(unpack_rle(&table, h, row_bytes, format.big)?);
            }
            planes
        }
        _ => return Err(FileError::Corrupt("統合画像の知らない圧縮")),
    };
    let to_8bit = |plane: &[u8]| -> Vec<u8> {
        if format.depth == 16 { plane.as_chunks::<2>().0.iter().map(|p| p[0]).collect() } else { plane.to_vec() }
    };
    let planes: Vec<Vec<u8>> = planes.iter().map(|p| to_8bit(p)).collect();
    let color_count = if format.gray { 1 } else { 3 };
    let plane = |i: usize| planes.get(i).map(|p| p.as_slice());
    let color = if format.gray { [plane(0); 3] } else { [plane(0), plane(1), plane(2)] };
    let alpha = if count > color_count { plane(color_count) } else { None };
    Ok(pixels_layer(bounds, color, alpha, bounds))
}

/// big endian の数を順に読む。
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Reader { data, at: 0 }
    }

    fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.at
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], FileError> {
        let end = self
            .at
            .checked_add(len)
            .filter(|&e| e <= self.data.len())
            .ok_or(FileError::Corrupt("ファイルが途中で終わっている"))?;
        let out = &self.data[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, FileError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, FileError> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().expect("2 バイト")))
    }

    fn u32(&mut self) -> Result<u32, FileError> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().expect("4 バイト")))
    }

    fn i32(&mut self) -> Result<i32, FileError> {
        Ok(self.u32()? as i32)
    }

    fn u64(&mut self) -> Result<u64, FileError> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().expect("8 バイト")))
    }

    /// PSB では 8 バイト、PSD では 4 バイトの長さ。
    fn length(&mut self, big: bool) -> Result<usize, FileError> {
        Ok(if big { self.u64()? as usize } else { self.u32()? as usize })
    }
}

#[cfg(test)]
mod tests;
