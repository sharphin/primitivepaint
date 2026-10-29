use std::collections::HashMap;

/// UI の文字に使うフォント。OS に入っている日本語フォントを読み込み、ラスタライズ結果をキャッシュする。
pub struct Font {
    font: fontdue::Font,
    cache: HashMap<(char, u32), Glyph>,
}

/// ラスタライズ済みの 1 文字。`coverage` は幅 × 高さの 8bit 被覆率。
pub struct Glyph {
    pub metrics: fontdue::Metrics,
    pub coverage: Vec<u8>,
}

/// 探すフォントの場所と、TTC 内の番号。先に見つかったものを使う。
const SYSTEM_FONTS: [(&str, u32); 6] = [
    (r"C:\Windows\Fonts\meiryo.ttc", 2), // Meiryo UI
    (r"C:\Windows\Fonts\YuGothM.ttc", 0),
    (r"C:\Windows\Fonts\msgothic.ttc", 0),
    ("/usr/share/fonts/truetype/fonts-japanese-gothic.ttf", 0),
    ("/usr/share/fonts/opentype/ipafont-gothic/ipagp.ttf", 0),
    ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
];

impl Font {
    /// OS の日本語フォントを読み込む。見つからなければ `None`（文字なしで UI を描く）。
    pub fn load_system() -> Option<Font> {
        let font = SYSTEM_FONTS.iter().find_map(|&(path, index)| {
            let bytes = std::fs::read(path).ok()?;
            Font::from_bytes(&bytes, index)
        });
        if font.is_none() {
            log::warn!("日本語フォントが見つからないため、UI の文字を表示しません");
        }
        font
    }

    pub fn from_bytes(bytes: &[u8], collection_index: u32) -> Option<Font> {
        let settings = fontdue::FontSettings { collection_index, ..Default::default() };
        let font = fontdue::Font::from_bytes(bytes, settings).ok()?;
        Some(Font { font, cache: HashMap::new() })
    }

    pub fn glyph(&mut self, c: char, px: f32) -> &Glyph {
        let font = &self.font;
        self.cache.entry((c, px.to_bits())).or_insert_with(|| {
            let (metrics, coverage) = font.rasterize(c, px);
            Glyph { metrics, coverage }
        })
    }

    /// 文字の大きさ `px` のときの (ascent, descent)。descent は負の値。
    pub fn line_metrics(&self, px: f32) -> (f32, f32) {
        self.font.horizontal_line_metrics(px).map_or((px * 0.8, -px * 0.2), |m| (m.ascent, m.descent))
    }

    pub fn measure(&mut self, text: &str, px: f32) -> f32 {
        text.chars().map(|c| self.glyph(c, px).metrics.advance_width).sum()
    }
}
