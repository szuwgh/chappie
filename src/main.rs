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
use crate::handle::tui_retore;
use crate::util::mmap_file;
use chap::Chappie;
use clap::Parser;
use crossterm::execute;
use error::ChapResult;
use std::error::Error;
use tui::ChapTui;
fn main() -> Result<(), Box<dyn Error>> {
    //let original_hook = panic::take_hook();
    // panic::set_hook(Box::new(move |info| {
    //     // 你的自定义 panic 处理逻辑
    //     restore();
    //     original_hook(info);
    // }));
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
