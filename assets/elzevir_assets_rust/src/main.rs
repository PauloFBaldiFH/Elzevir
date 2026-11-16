//! Gera assets Elzevir em SVG, PNG e ICO exclusivamente a partir de código Rust.
//! Os PNGs e ICOs usam as crates `resvg` e `ico`; não carregamos imagens externas.

mod assets;

use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use ico::{IconDir, IconDirEntry, IconImage, ResourceType};
use resvg::{tiny_skia, usvg};

type DynError = Box<dyn Error>;
const ICO_SIZES: &[u32] = &[16, 20, 24, 32, 40, 48, 64, 128, 256];
const PNG_SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256, 512];

fn main() -> Result<(), DynError> {
    let mut args = env::args().skip(1);
    let mut output = PathBuf::from("generated-assets");
    let mut only: Option<String> = None;
    let mut svg_only = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                output = PathBuf::from(args.next().ok_or("faltou caminho após --out")?);
            }
            "--only" => {
                only = Some(args.next().ok_or("faltou nome após --only")?);
            }
            "--svg-only" => svg_only = true,
            "--help" | "-h" => {
                help();
                return Ok(());
            }
            _ => return Err(format!("argumento desconhecido: {arg}; use --help").into()),
        }
    }
    fs::create_dir_all(&output)?;

    let mascots: &[(&str, fn() -> String)] = &[
        ("caranguejo_bibliotecario", assets::caranguejo_bibliotecario::svg),
        ("caranguejo_bibliotecario_recortado", assets::caranguejo_bibliotecario::svg_recortado),
        ("caranguejo_leitor", assets::caranguejo_leitor::svg),
        ("caranguejo_leitor_com_fundo", assets::caranguejo_leitor::svg_com_fundo),
        ("elzevir_tray_icon", assets::elzevir_tray_icon::svg),
    ];

    if let Some(ref key) = only {
        if !mascots.iter().any(|(name, _)| name == key) {
            return Err(format!("personagem desconhecido: {key}").into());
        }
    }

    for &(name, svg_source) in mascots {
        if only.as_deref().is_some_and(|key| key != name) {
            continue;
        }
        let svg = svg_source();
        fs::write(output.join(format!("{name}.svg")), &svg)?;
        println!("SVG: {}", output.join(format!("{name}.svg")).display());
        if svg_only {
            continue;
        }
        // Rasterização feita em memória, sem abrir SVG/PNG externo.
        let tree = usvg::Tree::from_str(&svg, &usvg::Options::default())?;
        for &size in PNG_SIZES {
            let pixmap = rasterize(&tree, size)?;
            let filename = format!("{name}_{size}.png");
            pixmap.save_png(output.join(filename))?;
        }
        // Ícones multi-tamanho para usar em aplicativos Windows e bandeja.
        write_ico(&tree, &output.join(format!("{name}.ico")))?;
        println!("PNG + ICO: {name}");
    }
    Ok(())
}

fn rasterize(tree: &usvg::Tree, size: u32) -> Result<tiny_skia::Pixmap, DynError> {
    let mut pixmap = tiny_skia::Pixmap::new(size, size).ok_or("resolução inválida")?;
    // Preserva aspect ratio e centraliza imagens retangulares numa tela quadrada.
    let width = tree.size().width();
    let height = tree.size().height();
    let factor = (size as f32 / width).min(size as f32 / height);
    let translate_x = (size as f32 - width * factor) / 2.0;
    let translate_y = (size as f32 - height * factor) / 2.0;
    let transform = tiny_skia::Transform::from_row(factor, 0.0, 0.0, factor, translate_x, translate_y);
    resvg::render(tree, transform, &mut pixmap.as_mut());
    Ok(pixmap)
}

fn write_ico(tree: &usvg::Tree, filename: &Path) -> Result<(), DynError> {
    let mut icons = IconDir::new(ResourceType::Icon);
    for &size in ICO_SIZES {
        let pixmap = rasterize(tree, size)?;
        // tiny-skia armazena RGBA pré-multiplicado. ICO requer RGBA normal.
        // `take_demultiplied` evita as bordas escuras com transparência.
        let rgba = pixmap.take_demultiplied();
        let image = IconImage::from_rgba_data(size, size, rgba);
        icons.add_entry(IconDirEntry::encode(&image)?);
    }
    icons.write(fs::File::create(filename)?)?;
    Ok(())
}

fn help() {
    println!("Elzevir assets (Rust)\n\n\
USO:\n  cargo run --release -- [--out caminho] [--svg-only] [--only nome]\n\n\
NOMES:\n  caranguejo_bibliotecario\n  caranguejo_bibliotecario_recortado\n  caranguejo_leitor\n  caranguejo_leitor_com_fundo\n  elzevir_tray_icon");
}
