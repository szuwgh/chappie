#![feature(variant_count)]
#![feature(async_closure)]
#![feature(let_chains)]
#![feature(trait_alias)]
//mod chatapi;
mod byteutil;
mod chap;
mod cli;
mod command;
mod editor;
mod error;
mod function;
mod fuzzy;
mod gap_buffer;
mod handle;
mod lua;
mod pg;
mod plugin;
mod textwarp;
mod tui;
mod util;
mod vb;
use crate::cli::Cli;
use crate::handle::tui_retore;
use crate::util::mmap_file;
use chap::Chappie;
use clap::Parser;
use crossterm::execute;
use error::ChapResult;
use std::error::Error;
use tui::ChapTui;
fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let filename = cli.get_filepath()?;
    //校验文件是否存在
    if !std::path::Path::new(filename).exists() {
        return Err(error::ChapError::FileNotFound(filename.to_string()).into());
    }
    if atty::is(atty::Stream::Stdin) {
        if let Err(e) = run_app(&cli, filename) {
            println!("chap error: {}", e);
        }
    }
    tui_retore()?;
    Ok(())
}

fn run_app(cli: &Cli, filename: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut chap = Chappie::new(&cli)?;
    chap.run(filename)?;
    Ok(())
}
