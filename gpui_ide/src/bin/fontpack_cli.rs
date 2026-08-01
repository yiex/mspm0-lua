#[path = "../compile.rs"]
mod compile;
#[path = "../fontpack.rs"]
mod fontpack;

use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let output = PathBuf::from(args.next().ok_or(
        "usage: fontpack-cli OUTPUT.luac FONT.ttf SOURCE.lua [SOURCE.lua ...]",
    )?);
    let font = PathBuf::from(args.next().ok_or(
        "usage: fontpack-cli OUTPUT.luac FONT.ttf SOURCE.lua [SOURCE.lua ...]",
    )?);
    let sources = args.map(PathBuf::from).collect::<Vec<_>>();
    if sources.is_empty() {
        return Err("at least one Lua source is required".into());
    }

    let mut combined = String::new();
    for source in &sources {
        combined.push_str(&std::fs::read_to_string(source)?);
        combined.push('\n');
    }
    let font = font
        .to_str()
        .ok_or("font path is not valid UTF-8")?;
    let Some((message, generated)) =
        fontpack::build_oled_font_module(&combined, font, font)?
    else {
        return Err("the supplied sources do not use the OLED API".into());
    };
    let bytecode = compile::compile_source(Path::new("in-process"), &generated)?;
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&output, &bytecode)?;
    println!("{message}");
    println!("{} {} bytes", output.display(), bytecode.len());
    Ok(())
}
