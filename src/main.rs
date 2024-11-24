#![feature(variant_count)]
#![feature(async_closure)]
use chap::Chappie;
use clap::Parser;
use error::ChapResult;
use tui::ChapTui;
mod chatapi;
use once_cell::sync::Lazy;
mod error;
mod fuzzy;
mod vb;

mod chap;
mod cmd;
mod text;
mod tui;
use crate::cmd::Cli;
use crate::util::map_file;
mod util;

fn main() -> ChapResult<()> {
    let cli = Cli::parse();
    let mmap = map_file(cli.get_filepath())?;
    let mut chap = Chappie::new(&cli)?;
    chap.run(mmap)?;
    Ok(())
}
