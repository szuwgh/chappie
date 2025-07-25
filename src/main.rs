#![feature(variant_count)]
#![feature(async_closure)]
#![feature(let_chains)]
#![feature(trait_alias)]
//mod chatapi;
mod error;
mod fuzzy;
mod vb;
use std::io::{self, Read};
mod byteutil;
mod chap;
mod cli;
mod command;
mod editor;
mod gap_buffer;
mod handle;
mod textwarp;
mod tui;
mod util;
use crate::cli::Cli;
use crate::util::mmap_file;
use chap::Chappie;
use clap::Parser;
use error::ChapResult;
use ratatui::init;
use ratatui::restore;
use std::error::Error;
use tui::ChapTui;
fn main() -> Result<(), Box<dyn Error>> {
    color_eyre::install()?; // 安装 color_eyre 错误处理
    let cli = Cli::parse();
    if atty::is(atty::Stream::Stdin) {
        let mut chap = Chappie::new(&cli)?;
        chap.run(cli.get_filepath()?)?;
    }
    restore();
    Ok(())
}
