#![feature(variant_count)]
#![feature(async_closure)]
#![feature(let_chains)]
#![feature(trait_alias)]
mod chatapi;
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
use tui::ChapTui;

fn main() -> ChapResult<()> {
    let cli = Cli::parse();
    if atty::is(atty::Stream::Stdin) {
        let mut chap = Chappie::new(&cli)?;
        chap.run(cli.get_filepath()?)?;
    }
    Ok(())
}
