#![feature(variant_count)]
#![feature(async_closure)]
#![feature(let_chains)]
mod chatapi;
mod error;
mod fuzzy;
mod vb;
use std::io::{self, Read};
mod chap;
mod cmd;
mod edit;
mod gap_buffer;
mod text;
mod tui;
mod util;
use crate::cmd::Cli;
use crate::util::map_file;
use chap::Chappie;
use clap::Parser;
use error::ChapResult;
use tui::ChapTui;

fn main() -> ChapResult<()> {
    let cli = Cli::parse();
    if atty::is(atty::Stream::Stdin) {
        // let mmap = map_file(cli.get_filepath()?)?;
        let mut chap = Chappie::new(&cli)?;
        chap.run2(cli.get_filepath()?)?;
    } else {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        let mut chap = Chappie::new(&cli)?;
        chap.run(buffer)?;
    }
    Ok(())
}
