use super::*;
use pp_core::SheetMaterial;

/// マスク（範囲, 既定の濃さ, 各画素の濃さ）。
type TestMask = (Rect, u8, fn(i32, i32) -> u8);

/// テスト用のレイヤー。画素は premultiplied ではない RGBA。
struct TestLayer {
    name: &'static str,
    rect: Rect,
    pixel: fn(i32, i32) -> [u8; 4],
    opacity: u8,
    key: &'static [u8; 4],
    clip: bool,
    hidden: bool,
    mask: Option<TestMask>,
    section: Option<u32>,
}

impl TestLayer {
    fn new(name: &'static str, rect: Rect, pixel: fn(i32, i32) -> [u8; 4]) -> Self {
        TestLayer {
            name,
            rect,
            pixel,
            opacity: 255,
            key: b"norm",
            clip: false,
            hidden: false,
            mask: None,
            section: None,
        }
    }

    /// フォルダの印（終わりの印か本体）。
    fn section(name: &'static str, kind: u32, key: &'static [u8; 4]) -> Self {
        TestLayer { section: Some(kind), key, ..TestLayer::new(name, Rect::EMPTY, |_, _| [0; 4]) }
    }
}

/// 1 チャンネル分（`rle` なら PackBits、そうでなければ無圧縮）。
fn channel_bytes(values: &[u8], width: usize, rle: bool) -> Vec<u8> {
    let mut out = Vec::new();
    if !rle || width == 0 {
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(values);
        return out;
    }
    out.extend_from_slice(&1u16.to_be_bytes());
    let rows: Vec<Vec<u8>> = values.chunks(width).map(pack_bits).collect();
    for row in &rows {
        out.extend_from_slice(&(row.len() as u16).to_be_bytes());
    }
    for row in rows {
        out.extend_from_slice(&row);
    }
    out
}

/// 同じ値の続く所は繰り返し、そうでない所はそのまま並べる、簡単な PackBits。
fn pack_bits(row: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < row.len() {
        let run = row[i..].iter().take(128).take_while(|&&v| v == row[i]).count();
        if run >= 2 {
            out.push((1 - run as i32) as i8 as u8);
            out.push(row[i]);
            i += run;
        } else {
            let len = row[i..].len().min(128);
            let literal = (1..len).find(|&k| k + 1 < row[i..].len() && row[i + k] == row[i + k + 1]).unwrap_or(len);
            out.push((literal - 1) as u8);
            out.extend_from_slice(&row[i..i + literal]);
            i += literal;
        }
    }
    out
}

fn psd(width: u32, height: u32, layers: &[TestLayer], rle: bool) -> Vec<u8> {
    let mut records = Vec::new();
    let mut image = Vec::new();
    for layer in layers {
        let (w, h) = (layer.rect.width() as usize, layer.rect.height() as usize);
        let mut planes: Vec<(i16, Vec<u8>)> = Vec::new();
        for (id, c) in [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)] {
            let mut values = Vec::with_capacity(w * h);
            for y in layer.rect.y0..layer.rect.y1 {
                for x in layer.rect.x0..layer.rect.x1 {
                    values.push((layer.pixel)(x, y)[c]);
                }
            }
            planes.push((id, channel_bytes(&values, w, rle)));
        }
        if let Some((rect, _, value)) = layer.mask {
            let mut values = Vec::new();
            for y in rect.y0..rect.y1 {
                for x in rect.x0..rect.x1 {
                    values.push(value(x, y));
                }
            }
            planes.push((-2, channel_bytes(&values, rect.width() as usize, rle)));
        }
        let rect_bytes = |out: &mut Vec<u8>, r: Rect| {
            for v in [r.y0, r.x0, r.y1, r.x1] {
                out.extend_from_slice(&v.to_be_bytes());
            }
        };
        let rect = if layer.rect.is_empty() { Rect { x0: 0, y0: 0, x1: 0, y1: 0 } } else { layer.rect };
        rect_bytes(&mut records, rect);
        records.extend_from_slice(&(planes.len() as u16).to_be_bytes());
        for (id, data) in &planes {
            records.extend_from_slice(&id.to_be_bytes());
            records.extend_from_slice(&(data.len() as u32).to_be_bytes());
        }
        records.extend_from_slice(b"8BIM");
        records.extend_from_slice(layer.key);
        records.push(layer.opacity);
        records.push(layer.clip as u8);
        records.push(if layer.hidden { 0x02 } else { 0 } | 0x08);
        records.push(0);
        let mut extra = Vec::new();
        match layer.mask {
            Some((rect, fill, _)) => {
                extra.extend_from_slice(&20u32.to_be_bytes());
                rect_bytes(&mut extra, rect);
                extra.extend_from_slice(&[fill, 0, 0, 0]);
            }
            None => extra.extend_from_slice(&0u32.to_be_bytes()),
        }
        extra.extend_from_slice(&0u32.to_be_bytes());
        // 名前（パスカル文字列、4 バイト単位）は ASCII の仮の名前にして、本当の名前は luni に入れる
        let ascii = b"layer";
        extra.push(ascii.len() as u8);
        extra.extend_from_slice(ascii);
        while extra.len() % 4 != 0 {
            extra.push(0);
        }
        let units: Vec<u16> = layer.name.encode_utf16().collect();
        let mut luni = (units.len() as u32).to_be_bytes().to_vec();
        units.iter().for_each(|u| luni.extend_from_slice(&u.to_be_bytes()));
        if !luni.len().is_multiple_of(2) {
            luni.push(0);
        }
        extra.extend_from_slice(b"8BIMluni");
        extra.extend_from_slice(&(luni.len() as u32).to_be_bytes());
        extra.extend_from_slice(&luni);
        if let Some(kind) = layer.section {
            let mut lsct = kind.to_be_bytes().to_vec();
            lsct.extend_from_slice(b"8BIM");
            lsct.extend_from_slice(layer.key);
            extra.extend_from_slice(b"8BIMlsct");
            extra.extend_from_slice(&(lsct.len() as u32).to_be_bytes());
            extra.extend_from_slice(&lsct);
        }
        records.extend_from_slice(&(extra.len() as u32).to_be_bytes());
        records.extend_from_slice(&extra);
        for (_, data) in planes {
            image.extend_from_slice(&data);
        }
    }
    let mut info = (layers.len() as u16).to_be_bytes().to_vec();
    info.extend_from_slice(&records);
    info.extend_from_slice(&image);
    if !info.len().is_multiple_of(2) {
        info.push(0);
    }
    let mut section = (info.len() as u32).to_be_bytes().to_vec();
    section.extend_from_slice(&info);
    section.extend_from_slice(&0u32.to_be_bytes());

    let mut out = b"8BPS".to_vec();
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&8u16.to_be_bytes());
    out.extend_from_slice(&MODE_RGB.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&(section.len() as u32).to_be_bytes());
    out.extend_from_slice(&section);
    // 統合画像（白、無圧縮）
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend(std::iter::repeat_n(255u8, (width * height * 3) as usize));
    out
}

const RED: [u8; 4] = [255, 0, 0, 255];
const GRAY: [u8; 4] = [128, 128, 128, 255];

fn whole(w: i32, h: i32) -> Rect {
    Rect::new(0, 0, w, h)
}

#[test]
fn layers_become_sheets_with_names_and_baked_opacity() {
    let mut top = TestLayer::new("上の線", Rect::new(2, 3, 6, 5), |_, _| RED);
    top.opacity = 128;
    let file = psd(10, 8, &[TestLayer::new("下塗り", whole(10, 8), |_, _| GRAY), top], false);
    let imported = load(&file).unwrap();
    let doc = &imported.document;
    assert_eq!((doc.width, doc.height), (10, 8));
    assert_eq!(doc.layers.len(), 3);
    assert_eq!(doc.layers[1].name, "用紙（下塗り）");
    assert_eq!(doc.layers[1].pixel(9, 7), Rgba8 { r: 128, g: 128, b: 128, a: 255 });
    assert_eq!(doc.layers[2].name, "上の線");
    assert_eq!(doc.layers[2].material, SheetMaterial::ClearFilm);
    assert_eq!(doc.layers[2].pixel(2, 3), Rgba8 { r: 128, g: 0, b: 0, a: 128 });
    assert_eq!(doc.layers[2].pixel(6, 3).a, 0, "範囲の外は透明");
}

#[test]
fn rle_and_raw_read_the_same() {
    let layers = || [TestLayer::new("模様", whole(300, 20), |x, y| [(x % 256) as u8, (y * 9) as u8, 7, 200])];
    let raw = load(&psd(300, 20, &layers(), false)).unwrap();
    let rle = load(&psd(300, 20, &layers(), true)).unwrap();
    for (x, y) in [(0, 0), (299, 19), (150, 10), (255, 3)] {
        assert_eq!(raw.document.layers[1].pixel(x, y), rle.document.layers[1].pixel(x, y), "({x}, {y})");
    }
    let px = raw.document.layers[1].pixel(10, 2);
    assert_eq!(px.a, 200);
    assert_eq!(px.r, ((10 * 200 + 127) / 255) as u8);
}

#[test]
fn folders_multiply_hidden_and_mask() {
    let mut shadow = TestLayer::new("影", whole(4, 4), |_, _| GRAY);
    shadow.key = b"mul ";
    let mut hidden = TestLayer::new("下書き", whole(4, 4), |_, _| GRAY);
    hidden.hidden = true;
    let mut masked = TestLayer::new("マスク付き", whole(4, 4), |_, _| RED);
    masked.mask = Some((Rect::new(0, 0, 2, 4), 0, |_, _| 255));
    let layers = [
        TestLayer::new("色", whole(4, 4), |_, _| RED),
        // 通過のフォルダ：終わりの印、中身、本体
        TestLayer::section("</Layer group>", SECTION_END, b"norm"),
        shadow,
        TestLayer::section("影のフォルダ", SECTION_OPEN, b"pass"),
        hidden,
        masked,
    ];
    let imported = load(&psd(4, 4, &layers, true)).unwrap();
    let doc = &imported.document;
    // 用紙（色に影を統合）、下書き（非表示）、マスク付き
    assert_eq!(doc.layers.len(), 4, "{:?}", imported.notes);
    let paper = doc.layers[1].pixel(0, 0);
    assert!((127..=129).contains(&paper.r) && paper.g == 0, "影は乗算で統合: {paper:?}");
    assert!(!doc.layers[2].visible);
    assert_eq!(doc.layers[3].pixel(0, 0).a, 255);
    assert_eq!(doc.layers[3].pixel(3, 0).a, 0, "マスクの外（既定の濃さ 0）");
}

#[test]
fn normal_folder_becomes_one_sheet() {
    let layers = [
        TestLayer::section("</Layer group>", SECTION_END, b"norm"),
        TestLayer::new("a", whole(4, 4), |_, _| RED),
        TestLayer::new("b", Rect::new(0, 0, 2, 2), |_, _| GRAY),
        TestLayer::section("まとめ", SECTION_CLOSED, b"norm"),
    ];
    let imported = load(&psd(4, 4, &layers, false)).unwrap();
    let doc = &imported.document;
    assert_eq!(doc.layers.len(), 2);
    assert_eq!(doc.layers[1].pixel(0, 0), Rgba8 { r: 128, g: 128, b: 128, a: 255 });
    assert_eq!(doc.layers[1].pixel(3, 3), Rgba8 { r: 255, g: 0, b: 0, a: 255 });
}

#[test]
fn file_without_layers_uses_the_merged_image() {
    let file = psd(5, 5, &[], false);
    // レイヤー情報を空にした PSD
    let imported = load(&file).unwrap();
    assert_eq!(imported.document.layers[1].pixel(4, 4), Rgba8 { r: 255, g: 255, b: 255, a: 255 });
}

#[test]
fn not_a_psd_and_cmyk_are_refused() {
    assert!(matches!(load(b"hello world, not a psd"), Err(FileError::NotOurFile)));
    let mut cmyk = psd(2, 2, &[], false);
    cmyk[24..26].copy_from_slice(&4u16.to_be_bytes());
    assert!(matches!(load(&cmyk), Err(FileError::Unsupported(_))));
}

#[test]
fn packbits_runs_and_literals() {
    let row = [1u8, 1, 1, 2, 3, 4, 4, 5];
    let packed = pack_bits(&row);
    let mut body = (packed.len() as u16).to_be_bytes().to_vec();
    body.extend_from_slice(&packed);
    assert_eq!(unpack_rle(&body, 1, row.len(), false).unwrap(), row);
}
