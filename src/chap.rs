use crate::chatapi::grop::GroqApi;
use crate::chatapi::LlmClient;
use crate::cmd::Cli;
use crate::text::SimpleText;
use crate::tui::UIType;
use crate::util::map_file;
use crate::ChapResult;
use crate::ChapTui;
use fastembed::EmbeddingModel;
use fastembed::InitOptions;
use fastembed::TextEmbedding;
use llmapi::LlmApi;
use memmap2::Mmap;
use once_cell::sync::Lazy;
use simplelog::*;
use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::runtime::Builder;
use tokio::sync::mpsc;
use vectorbase::ann::AnnType;
use vectorbase::collection::Collection;
use vectorbase::config::ConfigBuilder;
use vectorbase::schema::Document;
use vectorbase::schema::FieldEntry;
use vectorbase::schema::Schema;
use vectorbase::schema::TensorEntry;
use vectorbase::schema::Vector;
use vectorbase::schema::VectorEntry;
use vectorbase::schema::VectorType;
// 单例的 Tokio runtime
pub(crate) static RUNTIME: Lazy<tokio::runtime::Runtime> = Lazy::new(|| {
    Builder::new_multi_thread()
        .enable_io()
        .enable_time() // Enable time (timers)
        .worker_threads(2)
        .build()
        .expect("Failed to create runtime")
});

//app
pub(crate) struct Chappie {
    tui: ChapTui,
    vb: Collection,
}

impl Chappie {
    pub(crate) fn new(cli: &Cli) -> ChapResult<Chappie> {
        // 配置日志输出到文件
        WriteLogger::init(
            LevelFilter::Debug,          // 设置日志级别
            Config::default(),           // 使用默认日志配置
            File::create("output.log")?, // 创建日志文件
        )?;

        let llm_client = LlmClient::new(cli.get_llm(), cli.get_api_key(), cli.get_model())?;
        let (prompt_tx, prompt_rx) = mpsc::channel::<String>(1);
        let (llm_res_tx, llm_res_rx) = mpsc::channel::<String>(1);
        let embed = Arc::new(TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2)
                .with_show_download_progress(true)
                .with_cache_dir(PathBuf::from("./model")),
        )?);
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
        let vb = Collection::new(schema.clone(), config)?;
        let chap_ui = ChapTui::new(
            prompt_tx,
            vb.clone(),
            embed.clone(),
            llm_res_rx,
            UIType::Lite,
        )?;
        let vb_c = vb.clone();
        RUNTIME.spawn(async move {
            request_llm(
                prompt_rx,
                llm_res_tx,
                vb_c,
                embed.clone(),
                schema,
                llm_client,
            )
            .await;
        });
        // todo!()
        Ok(Self {
            tui: chap_ui,
            vb: vb,
        })
    }

    pub(crate) fn run<T: SimpleText>(&mut self, bytes: T) -> ChapResult<()> {
        RUNTIME.block_on(async move { self.tui.render(bytes).await })
    }
}

async fn request_llm(
    mut prompt_rx: mpsc::Receiver<String>,
    llm_res_tx: mpsc::Sender<String>,
    vb: Collection,
    embed_model: Arc<TextEmbedding>,
    schema: Schema,
    mut llm_client: LlmClient,
) {
    let field_id_prompt = schema.get_field("prompt").unwrap();
    loop {
        let field_id_answer = schema.get_field("answer").unwrap();
        tokio::select! {
           Some(prompt) = prompt_rx.recv() => {
            match llm_client.request(&prompt).await {
                Ok(res)=>{
                    let embeddings = embed_model.embed(vec![&prompt], None).unwrap();
                    let mut d = Document::new();
                    d.add_text(field_id_prompt, &prompt);
                    d.add_text(field_id_answer, &res);
                    let v6 = Vector::from_slice(&embeddings[0], d);
                    // 插入向量数据库
                    vb.add(v6).await.unwrap();
                    llm_res_tx.send(res).await.unwrap();
                }
                Err(e)=>{
                    llm_res_tx.send(e.to_string()).await.unwrap();
                }
            }

              //通知ui线程更新

            }
        }
    }
}
