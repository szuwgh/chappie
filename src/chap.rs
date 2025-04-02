use crate::chatapi::LlmClient;
use crate::cmd::Cli;
use crate::text::SimpleText;
use crate::ChapResult;
use crate::ChapTui;
use std::path::Path;
////use fastembed::EmbeddingModel;
//use fastembed::InitOptions;
//use fastembed::TextEmbedding;
use once_cell::sync::Lazy;
use simplelog::*;
use std::fs;
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

const LLM_MODEL_DIR: &'static str = "/etc/chappie/model";
const CHAP_VB_DIR: &'static str = "/etc/chappie/data";
const CHAP_LOG_DIR: &'static str = "/var/log/chap";

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
    vdb: Option<Collection>,
}

impl Chappie {
    pub(crate) fn new(cli: &Cli) -> ChapResult<Chappie> {
        fs::create_dir_all(LLM_MODEL_DIR)?;
        fs::create_dir_all(CHAP_VB_DIR)?;
        fs::create_dir_all(CHAP_LOG_DIR)?;
        // 配置日志输出到文件
        WriteLogger::init(
            LevelFilter::Debug,                                          // 设置日志级别
            Config::default(),                                           // 使用默认日志配置
            File::create(PathBuf::from(CHAP_LOG_DIR).join("chap.log"))?, // 创建日志文件
        )?;
        // let embed = Arc::new(TextEmbedding::try_new(
        //     InitOptions::new(EmbeddingModel::AllMiniLML6V2)
        //         .with_show_download_progress(true)
        //         .with_cache_dir(PathBuf::from(LLM_MODEL_DIR)),
        // )?);
        let llm_client = LlmClient::new(cli.get_llm(), cli.get_api_key(), cli.get_model())?;
        let (prompt_tx, prompt_rx) = mpsc::channel::<String>(1);
        let (llm_res_tx, llm_res_rx) = mpsc::channel::<String>(1);
        let mut schema = Schema::with_vector(VectorEntry::new(
            "vector",
            AnnType::HNSW,
            TensorEntry::new(1, [384], VectorType::F32),
        ));
        schema.add_field(FieldEntry::str("prompt"));
        schema.add_field(FieldEntry::str("answer"));
        let config = ConfigBuilder::default()
            .data_path(PathBuf::from(CHAP_VB_DIR))
            .collect_name("chap")
            .build();
        let vdb = if cli.get_vb() {
            Some(Collection::new(schema.clone(), config)?)
        } else {
            None
        };
        let chap_ui = ChapTui::new(
            prompt_tx,
            vdb.clone(),
            //  embed.clone(),
            llm_res_rx,
            cli.get_ui_type(),
            cli.get_que(),
        )?;
        let vb_c = vdb.clone();
        Self::req_llm(prompt_rx, llm_res_tx, vb_c, schema, llm_client);
        Ok(Self {
            tui: chap_ui,
            vdb: vdb,
        })
    }

    fn req_llm(
        prompt_rx: mpsc::Receiver<String>,
        llm_res_tx: mpsc::Sender<String>,
        vdb: Option<Collection>,
        //  embed_model: Arc<TextEmbedding>,
        schema: Schema,
        llm_client: LlmClient,
    ) {
        RUNTIME.spawn(async move {
            request_llm(prompt_rx, llm_res_tx, vdb, schema, llm_client).await;
        });
    }

    pub(crate) fn run2<P: AsRef<Path>>(&mut self, p: P) -> ChapResult<()> {
        RUNTIME.block_on(async move { self.tui.render2(p).await })
    }

    pub(crate) fn run<T: SimpleText>(&mut self, bytes: T) -> ChapResult<()> {
        RUNTIME.block_on(async move { self.tui.render(bytes).await })
    }
}

async fn request_llm(
    mut prompt_rx: mpsc::Receiver<String>,
    llm_res_tx: mpsc::Sender<String>,
    vdb: Option<Collection>,
    // embed_model: Arc<TextEmbedding>,
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
                    // if let Some(vb) = &vdb {
                    //     let embeddings = embed_model.embed(vec![&prompt], None).unwrap();
                    //     let mut d = Document::new();
                    //     d.add_text(field_id_prompt, &prompt);
                    //     d.add_text(field_id_answer, &res);
                    //     let v6 = Vector::from_slice(&embeddings[0], d).unwrap();
                    //     // 插入向量数据库
                    //     vb.add(v6).await.unwrap();
                    // }
                    llm_res_tx.send(res).await.unwrap();
                }
                Err(e)=>{
                    llm_res_tx.send(e.to_string()).await.unwrap();
                }
            }

            }
        }
    }
}
