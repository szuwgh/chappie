#![feature(variant_count)]
#![feature(async_closure)]
use clap::Parser;
use error::ChapResult;
use tui::ChapUI;
mod chatapi;
use once_cell::sync::Lazy;
use vectorbase::schema::Document;
mod error;
mod fuzzy;
use fastembed::InitOptions;
use vectorbase::schema::Vector;
mod text;
mod tui;
use fastembed::EmbeddingModel;
use fastembed::TextEmbedding;
use std::path::PathBuf;
use vectorbase::collection::Collection;
use vectorbase::config::ConfigBuilder;
use vectorbase::schema::FieldEntry;
use vectorbase::schema::TensorEntry;
use vectorbase::schema::VectorEntry;
use vectorbase::schema::VectorType;
mod util;
use crate::chatapi::grop::ApiGroq;
use crate::util::map_file;
use std::sync::Arc;
use tokio::runtime::Builder;
use tokio::sync::mpsc;
use vectorbase::ann::AnnType;
use vectorbase::schema::Schema;
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

fn main() -> ChapResult<()> {
    //show_file();
    let file_path = "/root/pod.error.log";
    let mmap = map_file(file_path)?;
    let (prompt_tx, prompt_rx) = mpsc::channel::<String>(1);
    let (llm_res_tx, llm_res_rx) = mpsc::channel::<String>(1);

    let embed = Arc::new(
        TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2)
                .with_show_download_progress(true)
                .with_cache_dir(PathBuf::from("./model")),
        )
        .unwrap(),
    );

    let mut schema = Schema::with_vector(VectorEntry::new(
        "vector",
        AnnType::HNSW,
        TensorEntry::new(1, [384], VectorType::F32),
    ));
    schema.add_field(FieldEntry::str("prompt"));
    schema.add_field(FieldEntry::str("answer"));
    let config = ConfigBuilder::default()
        .data_path(PathBuf::from("./data"))
        .collect_name("chap")
        .build();
    let vb = Collection::new(schema.clone(), config).unwrap();
    let mut chap_ui = ChapUI::new(prompt_tx, vb.clone(), embed.clone())?;
    RUNTIME.spawn(async move {
        request_llm(prompt_rx, llm_res_tx, vb, embed.clone(), schema).await;
    });
    RUNTIME.block_on(async move {
        chap_ui.render(mmap, llm_res_rx).await.unwrap();
    });
    Ok(())
}

async fn request_llm(
    mut prompt_rx: mpsc::Receiver<String>,
    llm_res_tx: mpsc::Sender<String>,
    vb: Collection,
    embed_model: Arc<TextEmbedding>,
    schema: Schema,
) {
    let mut groq = ApiGroq::new("");
    let field_id_prompt = schema.get_field("prompt").unwrap();
    let field_id_answer = schema.get_field("answer").unwrap();
    loop {
        tokio::select! {
           Some(prompt) = prompt_rx.recv() => {
            let embeddings = embed_model.embed(vec![&prompt], None).unwrap();
            let mut d = Document::new();
            d.add_text(field_id_prompt, &prompt);
            let res = groq.request(prompt).await;
            d.add_text(field_id_answer, &res);
            let v6 = Vector::from_slice(&embeddings[0], d);
            // 插入向量数据库
            vb.add(v6).await.unwrap();
              //通知ui线程更新
            llm_res_tx.send(res).await.unwrap();
            }
        }
    }
}
