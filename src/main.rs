#![feature(variant_count)]
use clap::Parser;
use error::ChapResult;
use tui::ChapUI;
mod chatapi;
mod error;
mod fuzzy;
mod text;
mod tui;
mod util;
use crate::util::map_file;
/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[arg(short, long)]
    name: String,

    /// Number of times to greet
    #[arg(short, long, default_value_t = 1)]
    count: u8,
}

fn main() -> ChapResult<()> {
    //show_file();
    let file_path = "/opt/rsproject/chappie/vectorbase/src/disk.rs";
    let mmap = map_file(file_path)?;
    let mut chap_ui = ChapUI::new()?;
    chap_ui.render(&mmap)?;
    Ok(())
    //   show_file_from_mmap();
}
