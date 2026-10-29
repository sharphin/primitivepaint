//! .psd を読み込み、紙に変換した結果を見た目どおりに合成して PNG に書き出す（読み込みの確認用）。
//! `cargo run --release -p pp-file --example psd_to_png -- 入力.psd 出力.png`

use std::path::Path;

use pp_file::{OutputSize, png, psd};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, input, output] = args.as_slice() else {
        eprintln!("使い方: psd_to_png 入力.psd 出力.png");
        std::process::exit(2);
    };
    let imported = match psd::load_from_path(Path::new(input)) {
        Ok(imported) => imported,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    for sheet in imported.document.layers.iter().skip(1) {
        println!("紙: {}{}", sheet.name, if sheet.visible { "" } else { "（非表示）" });
    }
    for note in &imported.notes {
        println!("・{note}");
    }
    let size = OutputSize::of(&imported.document);
    if let Err(err) = png::export_to_path(Path::new(output), &imported.document, size, imported.document.dpi) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
