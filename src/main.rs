#![feature(variant_count)]
#![feature(async_closure)]
use clap::Parser;
use error::ChapResult;
use tui::ChapUI;
mod chatapi;
use once_cell::sync::Lazy;
mod error;
mod fuzzy;
mod text;
mod tui;
mod util;
use crate::chatapi::grop::ApiGroq;
use crate::util::map_file;
use tokio::runtime::Builder;
use tokio::signal::ctrl_c;
use tokio::sync::mpsc;
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

// 单例的 Tokio runtime
pub(crate) static RUNTIME: Lazy<tokio::runtime::Runtime> = Lazy::new(|| {
    Builder::new_multi_thread()
        .enable_io()
        .enable_time() // Enable time (timers)
        .worker_threads(2)
        .build()
        .expect("Failed to create runtime")
});

#[tokio::main]
async fn main() -> ChapResult<()> {
    //show_file();
    let file_path = "/opt/rsproject/chappie/vectorbase/src/disk.rs";
    let mmap = map_file(file_path)?;
    let (prompt_tx, prompt_rx) = mpsc::channel::<String>(1);
    let (llm_res_tx, llm_res_rx) = mpsc::channel::<String>(1);
    let mut chap_ui = ChapUI::new(prompt_tx)?;
    RUNTIME.spawn(async move {
        chap_ui.render(&mmap, llm_res_rx).await.unwrap();
    });
    RUNTIME.spawn(async move {
        request_llm(prompt_rx, llm_res_tx).await;
    });
    ctrl_c().await.expect("Failed to listen for Ctrl+C");
    Ok(())
}

async fn request_llm(mut prompt_rx: mpsc::Receiver<String>, llm_res_tx: mpsc::Sender<String>) {
    let mut groq = ApiGroq::new("");
    loop {
        tokio::select! {
           Some(prompt) = prompt_rx.recv() => {
              let res = groq.request(prompt).await;
              //通知ui线程更新
              llm_res_tx.send(res).await.unwrap();
            }
        }
    }
}
