//! このアプリ独自の作品ファイル（`.plim`）。
//!
//! 先頭に識別子と形式の版、続いてチャンク（4 バイトの種類 + 8 バイトの長さ + 中身）を並べる。
//! 数値はすべてリトルエンディアン。知らない種類のチャンクは読み飛ばすので、後から項目を足しても古い版で開ける。
//!
//! | チャンク | 中身 |
//! | --- | --- |
//! | `DOC ` | 幅・高さ・キャンバスモード・選んでいる紙・レイヤー数・解像度（dpi。古い版のファイルにはない） |
//! | `LAYR` | レイヤー 1 枚（下から順に並ぶ）。名前・素材・表示・不透明度・合成モードと、描いたタイル |
//! | `PALT` | パレットの絵の具（マスごとの反射率と量） |
//!
//! タイルとパレットのマスは zlib で圧縮する。何も描いていないタイルは書かない。

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

use miniz_oxide::deflate::compress_to_vec_zlib;
use miniz_oxide::inflate::decompress_to_vec_zlib_with_limit;
use pp_core::{BlendMode, CanvasMode, Document, Layer, Palette, Pigment, Rgba8, SheetMaterial, TILE_SIZE, Tile};

use crate::{FileError, Project};

/// 保存するときに付ける拡張子。
pub const EXTENSION: &str = "plim";

const MAGIC: &[u8; 8] = b"PLIM\r\n\x1a\n";
/// 形式の版。読めない変更をしたときだけ上げる（項目の追加はチャンクを足せば済む）。
const VERSION: u32 = 1;

const CHUNK_DOCUMENT: [u8; 4] = *b"DOC ";
const CHUNK_LAYER: [u8; 4] = *b"LAYR";
const CHUNK_PALETTE: [u8; 4] = *b"PALT";

const TILE_BYTES: usize = (TILE_SIZE * TILE_SIZE * 4) as usize;
/// パレット 1 マスのバイト数（有無 1 + 反射率 4 × 3 + 量 4）。
const PALETTE_CELL_BYTES: usize = 17;
/// 速さを優先した圧縮の強さ（0〜10）。描いたタイルは単色の部分が多く、低めでも十分縮む。
const COMPRESSION_LEVEL: u8 = 3;

/// `path` に保存する。いったん別名で書き終えてから置き換えるので、途中で失敗しても元のファイルは壊れない。
pub fn save_to_path(path: &Path, document: &Document, palette: &Palette) -> io::Result<()> {
    crate::write_replacing(path, |out| save(out, document, palette))
}

pub fn load_from_path(path: &Path) -> Result<Project, FileError> {
    let mut bytes = Vec::new();
    File::open(path)?.read_to_end(&mut bytes)?;
    load(&bytes)
}

/// 作業の状態を `out` に書き出す。
pub fn save(out: &mut impl Write, document: &Document, palette: &Palette) -> io::Result<()> {
    out.write_all(MAGIC)?;
    out.write_all(&VERSION.to_le_bytes())?;

    let mut doc = ChunkWriter::new(CHUNK_DOCUMENT);
    doc.u32(document.width);
    doc.u32(document.height);
    doc.u8(mode_code(document.mode));
    doc.u32(document.active_layer as u32);
    doc.u32(document.layers.len() as u32);
    // 後から足した項目。古い版は読まずに飛ばす
    doc.u32(document.dpi);
    doc.finish(out)?;

    for layer in &document.layers {
        layer_chunk(layer).finish(out)?;
    }
    palette_chunk(palette).finish(out)
}

/// `save` で書き出したバイト列を読み込む。
pub fn load(bytes: &[u8]) -> Result<Project, FileError> {
    let mut reader = ByteReader::new(bytes);
    if reader.take(MAGIC.len()).ok() != Some(MAGIC.as_slice()) {
        return Err(FileError::NotOurFile);
    }
    let version = reader.u32()?;
    if version > VERSION {
        return Err(FileError::NewerVersion(version));
    }

    let mut header = None;
    let mut layers = Vec::new();
    let mut palette = None;
    while !reader.is_empty() {
        let tag: [u8; 4] = reader.take(4)?.try_into().expect("4 バイト");
        let len = usize::try_from(reader.u64()?).map_err(|_| FileError::Corrupt("チャンクが大きすぎる"))?;
        let mut body = ByteReader::new(reader.take(len)?);
        match tag {
            CHUNK_DOCUMENT => header = Some(read_header(&mut body)?),
            CHUNK_LAYER => layers.push(read_layer(&mut body)?),
            CHUNK_PALETTE => palette = Some(read_palette(&mut body)?),
            _ => {} // 新しい版で増えたチャンクは読み飛ばす
        }
    }

    let header = header.ok_or(FileError::Corrupt("ドキュメントの情報がない"))?;
    if layers.len() != header.layer_count {
        return Err(FileError::Corrupt("レイヤーの数が合わない"));
    }
    let mut document = Document::from_layers(header.width, header.height, header.mode, layers, header.active_layer)
        .ok_or(FileError::Corrupt("レイヤーの構成がキャンバスモードに合わない"))?;
    if let Some(dpi) = header.dpi {
        document.dpi = dpi;
    }
    let palette = palette.ok_or(FileError::Corrupt("パレットがない"))?;
    Ok(Project { document, palette })
}

struct Header {
    width: u32,
    height: u32,
    mode: CanvasMode,
    active_layer: usize,
    layer_count: usize,
    /// 解像度を記録する前の版で保存したファイルでは `None`。
    dpi: Option<u32>,
}

fn read_header(body: &mut ByteReader) -> Result<Header, FileError> {
    Ok(Header {
        width: body.u32()?,
        height: body.u32()?,
        mode: mode_from_code(body.u8()?)?,
        active_layer: body.u32()? as usize,
        layer_count: body.u32()? as usize,
        dpi: if body.is_empty() { None } else { Some(body.u32()?.clamp(Document::MIN_DPI, Document::MAX_DPI)) },
    })
}

fn layer_chunk(layer: &Layer) -> ChunkWriter {
    let mut chunk = ChunkWriter::new(CHUNK_LAYER);
    chunk.string(&layer.name);
    chunk.u8(material_code(layer.material));
    chunk.u8(layer.visible as u8);
    chunk.u8(layer.opacity);
    chunk.u8(blend_code(layer.blend));

    // 保存のたびに中身が変わらないよう、タイルは位置の順に並べる
    let mut tiles: Vec<_> = layer.tiles().filter(|(_, tile)| !tile.is_transparent()).collect();
    tiles.sort_by_key(|((tx, ty), _)| (*ty, *tx));
    chunk.u32(tiles.len() as u32);
    let mut raw = Vec::with_capacity(TILE_BYTES);
    for ((tx, ty), tile) in tiles {
        raw.clear();
        raw.extend(tile.pixels().iter().flat_map(|p| [p.r, p.g, p.b, p.a]));
        chunk.i32(tx);
        chunk.i32(ty);
        chunk.compressed(&raw);
    }
    chunk
}

fn read_layer(body: &mut ByteReader) -> Result<Layer, FileError> {
    let name = body.string()?;
    let mut layer = Layer::with_material(name, material_from_code(body.u8()?)?);
    layer.visible = body.u8()? != 0;
    layer.opacity = body.u8()?;
    layer.blend = blend_from_code(body.u8()?)?;
    let tile_count = body.u32()?;
    for _ in 0..tile_count {
        let tx = body.i32()?;
        let ty = body.i32()?;
        let raw = body.compressed(TILE_BYTES)?;
        let pixels = raw.as_chunks::<4>().0.iter().map(|&[r, g, b, a]| Rgba8 { r, g, b, a }).collect();
        let tile = Tile::from_pixels(pixels).ok_or(FileError::Corrupt("タイルの大きさが違う"))?;
        layer.insert_tile(tx, ty, tile);
    }
    Ok(layer)
}

fn palette_chunk(palette: &Palette) -> ChunkWriter {
    let mut chunk = ChunkWriter::new(CHUNK_PALETTE);
    chunk.u32(palette.width());
    chunk.u32(palette.height());
    let mut raw = Vec::with_capacity((palette.width() * palette.height()) as usize * PALETTE_CELL_BYTES);
    for cell in palette.cells() {
        let (present, [r, g, b], amount) = match cell {
            Some((pigment, amount)) => (1u8, pigment.reflectance(), amount),
            None => (0, [0.0; 3], 0.0),
        };
        raw.push(present);
        for v in [r, g, b, amount] {
            raw.extend_from_slice(&v.to_le_bytes());
        }
    }
    chunk.compressed(&raw);
    chunk
}

fn read_palette(body: &mut ByteReader) -> Result<Palette, FileError> {
    let width = body.u32()?;
    let height = body.u32()?;
    let cell_count = width as usize * height as usize;
    let raw = body.compressed(cell_count * PALETTE_CELL_BYTES)?;
    let cells = raw
        .as_chunks::<PALETTE_CELL_BYTES>()
        .0
        .iter()
        .map(|cell| {
            let float = |i: usize| f32::from_le_bytes(cell[1 + i * 4..5 + i * 4].try_into().expect("4 バイト"));
            (cell[0] != 0).then(|| (Pigment::from_reflectance([float(0), float(1), float(2)]), float(3)))
        })
        .collect();
    Palette::from_cells(width, height, cells).ok_or(FileError::Corrupt("パレットの大きさが合わない"))
}

fn mode_code(mode: CanvasMode) -> u8 {
    match mode {
        CanvasMode::Paper => 0,
        CanvasMode::OilCanvas => 1,
    }
}

fn mode_from_code(code: u8) -> Result<CanvasMode, FileError> {
    match code {
        0 => Ok(CanvasMode::Paper),
        1 => Ok(CanvasMode::OilCanvas),
        _ => Err(FileError::Corrupt("知らないキャンバスモード")),
    }
}

fn material_code(material: SheetMaterial) -> u8 {
    match material {
        SheetMaterial::Paper => 0,
        SheetMaterial::TracingPaper => 1,
        SheetMaterial::ClearFilm => 2,
    }
}

fn material_from_code(code: u8) -> Result<SheetMaterial, FileError> {
    match code {
        0 => Ok(SheetMaterial::Paper),
        1 => Ok(SheetMaterial::TracingPaper),
        2 => Ok(SheetMaterial::ClearFilm),
        _ => Err(FileError::Corrupt("知らない紙の素材")),
    }
}

fn blend_code(blend: BlendMode) -> u8 {
    match blend {
        BlendMode::Normal => 0,
    }
}

fn blend_from_code(code: u8) -> Result<BlendMode, FileError> {
    match code {
        0 => Ok(BlendMode::Normal),
        _ => Err(FileError::Corrupt("知らない合成モード")),
    }
}

/// 1 つのチャンクの中身を組み立てる。
struct ChunkWriter {
    tag: [u8; 4],
    body: Vec<u8>,
}

impl ChunkWriter {
    fn new(tag: [u8; 4]) -> Self {
        ChunkWriter { tag, body: Vec::new() }
    }

    fn u8(&mut self, v: u8) {
        self.body.push(v);
    }

    fn u32(&mut self, v: u32) {
        self.body.extend_from_slice(&v.to_le_bytes());
    }

    fn i32(&mut self, v: i32) {
        self.body.extend_from_slice(&v.to_le_bytes());
    }

    fn string(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.body.extend_from_slice(s.as_bytes());
    }

    /// zlib で圧縮し、長さを付けて書く。
    fn compressed(&mut self, raw: &[u8]) {
        let packed = compress_to_vec_zlib(raw, COMPRESSION_LEVEL);
        self.u32(packed.len() as u32);
        self.body.extend_from_slice(&packed);
    }

    fn finish(self, out: &mut impl Write) -> io::Result<()> {
        out.write_all(&self.tag)?;
        out.write_all(&(self.body.len() as u64).to_le_bytes())?;
        out.write_all(&self.body)
    }
}

/// バイト列を先頭から読む。足りなければ `FileError::Corrupt` を返す。
struct ByteReader<'a> {
    bytes: &'a [u8],
}

impl<'a> ByteReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        ByteReader { bytes }
    }

    fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], FileError> {
        if len > self.bytes.len() {
            return Err(FileError::Corrupt("途中で終わっている"));
        }
        let (head, rest) = self.bytes.split_at(len);
        self.bytes = rest;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], FileError> {
        Ok(self.take(N)?.try_into().expect("N バイト"))
    }

    fn u8(&mut self) -> Result<u8, FileError> {
        Ok(self.array::<1>()?[0])
    }

    fn u32(&mut self) -> Result<u32, FileError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn i32(&mut self) -> Result<i32, FileError> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, FileError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    fn string(&mut self) -> Result<String, FileError> {
        let len = self.u32()? as usize;
        String::from_utf8(self.take(len)?.to_vec()).map_err(|_| FileError::Corrupt("名前が UTF-8 でない"))
    }

    /// `compressed` で書いた中身を展開する。展開後がちょうど `expected` バイトでなければ壊れている。
    fn compressed(&mut self, expected: usize) -> Result<Vec<u8>, FileError> {
        let len = self.u32()? as usize;
        let raw = decompress_to_vec_zlib_with_limit(self.take(len)?, expected)
            .map_err(|_| FileError::Corrupt("圧縮データを展開できない"))?;
        if raw.len() != expected {
            return Err(FileError::Corrupt("展開したデータの大きさが違う"));
        }
        Ok(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pp_core::{Color, Rect};
    use std::fs;

    fn sample() -> (Document, Palette) {
        let mut document = Document::new(600, 400, CanvasMode::Paper);
        document.layers[1].fill(Rect::new(10, 10, 300, 20), Rgba8 { r: 0, g: 0, b: 0, a: 255 });
        document.add_sheet(SheetMaterial::TracingPaper);
        document.active_layer_mut().set_pixel(-3, 399, Rgba8 { r: 10, g: 20, b: 30, a: 40 });
        document.flip_active_sheet();
        document.add_sheet(SheetMaterial::ClearFilm);
        document.select_sheet(false);
        let mut palette = Palette::new(40, 30);
        palette.put(Color { r: 0.1, g: 0.4, b: 0.9, a: 1.0 }, 10.0, 10.0, 5.0);
        (document, palette)
    }

    fn to_bytes(document: &Document, palette: &Palette) -> Vec<u8> {
        let mut bytes = Vec::new();
        save(&mut bytes, document, palette).unwrap();
        bytes
    }

    #[test]
    fn round_trip_keeps_sheets_pixels_and_palette() {
        let (document, palette) = sample();
        let project = load(&to_bytes(&document, &palette)).unwrap();
        let loaded = &project.document;

        assert_eq!((loaded.width, loaded.height, loaded.mode), (600, 400, CanvasMode::Paper));
        assert_eq!(loaded.active_layer, document.active_layer);
        assert_eq!(loaded.layers.len(), 4);
        for (a, b) in loaded.layers.iter().zip(&document.layers) {
            assert_eq!((&a.name, a.material, a.visible, a.opacity), (&b.name, b.material, b.visible, b.opacity));
        }
        assert_eq!(loaded.layers[0].pixel(599, 399), Rgba8 { r: 255, g: 255, b: 255, a: 255 });
        assert_eq!(loaded.layers[1].pixel(100, 15).a, 255);
        assert_eq!(loaded.layers[1].pixel(100, 25).a, 0);
        assert_eq!(loaded.layers[2].pixel(-3, 399), Rgba8 { r: 10, g: 20, b: 30, a: 40 });
        assert_eq!(loaded.layers[3].tile_count(), 0);

        assert!(project.palette.cells().eq(palette.cells()));
    }

    #[test]
    fn same_document_saves_to_same_bytes() {
        let (document, palette) = sample();
        assert_eq!(to_bytes(&document, &palette), to_bytes(&document, &palette));
    }

    #[test]
    fn dpi_round_trips_and_old_files_use_the_mode_default() {
        let (mut document, palette) = sample();
        document.dpi = 600;
        let bytes = to_bytes(&document, &palette);
        assert_eq!(load(&bytes).unwrap().document.dpi, 600);

        // 解像度を記録する前の版：DOC の中身に解像度の 4 バイトがない
        let header_start = MAGIC.len() + 4;
        let body_start = header_start + 12;
        let body_len = u64::from_le_bytes(bytes[header_start + 4..body_start].try_into().unwrap()) as usize;
        let mut old = bytes[..body_start + body_len - 4].to_vec();
        old[header_start + 4..body_start].copy_from_slice(&(body_len as u64 - 4).to_le_bytes());
        old.extend_from_slice(&bytes[body_start + body_len..]);
        assert_eq!(load(&old).unwrap().document.dpi, CanvasMode::Paper.default_dpi());
    }

    #[test]
    fn oil_canvas_round_trips() {
        let document = Document::new(64, 64, CanvasMode::OilCanvas);
        let project = load(&to_bytes(&document, &Palette::new(4, 4))).unwrap();
        assert_eq!(project.document.mode, CanvasMode::OilCanvas);
        assert_eq!(project.document.layers.len(), 2);
    }

    #[test]
    fn unknown_chunks_are_skipped() {
        let (document, palette) = sample();
        let mut bytes = to_bytes(&document, &palette);
        bytes.extend_from_slice(b"NEW!");
        bytes.extend_from_slice(&3u64.to_le_bytes());
        bytes.extend_from_slice(&[1, 2, 3]);
        assert!(load(&bytes).is_ok());
    }

    #[test]
    fn rejects_other_files_newer_versions_and_truncation() {
        assert!(matches!(load(b"not a paint file"), Err(FileError::NotOurFile)));

        let (document, palette) = sample();
        let mut newer = to_bytes(&document, &palette);
        newer[8..12].copy_from_slice(&(VERSION + 1).to_le_bytes());
        assert!(matches!(load(&newer), Err(FileError::NewerVersion(_))));

        let bytes = to_bytes(&document, &palette);
        for cut in [12, 30, bytes.len() / 2, bytes.len() - 1] {
            assert!(matches!(load(&bytes[..cut]), Err(FileError::Corrupt(_))), "{cut} バイトで切った");
        }
    }

    #[test]
    fn save_to_path_replaces_the_file() {
        let dir = std::env::temp_dir().join(format!("pp-file-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("作品.plim");
        let (document, palette) = sample();
        save_to_path(&path, &document, &palette).unwrap();
        save_to_path(&path, &Document::new(32, 32, CanvasMode::OilCanvas), &palette).unwrap();
        let project = load_from_path(&path).unwrap();
        assert_eq!(project.document.mode, CanvasMode::OilCanvas);
        assert!(!path.with_extension("plim.tmp").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
