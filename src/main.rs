use crate::tui::show_file;
use clap::Parser;
use std::io;
mod fuzzy;
mod text;
mod tui;
mod util;
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

fn main() {
    show_file();
    //   show_file_from_mmap();
}
