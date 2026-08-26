#![feature(variant_count)]
#![feature(async_closure)]
#![feature(let_chains)]
#![feature(trait_alias)]
#![feature(str_as_str)]
#![feature(target_feature_inline_always)]
//mod chatapi;
mod byteutil;
mod chap;
mod cli;
mod command;
mod common;
mod function;
mod fuzzy;
mod handle;
mod lua;
mod pg;
mod plugin;
mod searcher;
mod textwarp;
mod tui;
mod undo;
use crate::common::error::ChapError;
mod vb;
use crate::cli::Cli;
use crate::handle::tui_retore;
use crate::tui::RenderSource;
use chap::Chappie;
use clap::Parser;
use crossterm::execute;
use std::error::Error;

use tui::ChapTui;
fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    //初始化终端
    if atty::is(atty::Stream::Stdin) {
        let filename = cli.get_filepath()?;
        //校验文件是否存在
        let path_buf = std::path::PathBuf::from(filename);
        if !path_buf.exists() {
            return Err(ChapError::FileNotFound(filename.to_string()).into());
        }
        if path_buf.is_dir() {
            if let Err(e) = run_app(&cli, RenderSource::Dir(path_buf)) {
                println!("chap error: {}", e);
            }
        } else {
            if let Err(e) = run_app(&cli, RenderSource::File(path_buf)) {
                println!("chap error: {}", e);
            }
        }
    } else {
        let mut temp_file = tempfile::NamedTempFile::new()?;
        std::io::copy(&mut std::io::stdin(), &mut temp_file)?;
        if let Err(e) = run_app(&cli, RenderSource::Temp(temp_file)) {
            println!("chap error: {}", e);
        }
    }
    tui_retore()?;
    Ok(())
}

fn run_app(cli: &Cli, source: RenderSource) -> Result<(), Box<dyn std::error::Error>> {
    let mut chap = Chappie::new(&cli, &source)?;
    chap.run(source)?;
    Ok(())
}
