use crate::cli::Cli;
use crate::ChapResult;
use crate::ChapTui;
use once_cell::sync::Lazy;
use simplelog::*;
use std::fs;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;
use tokio::runtime::Builder;
use tokio::sync::mpsc;

// const LLM_MODEL_DIR: &'static str = "~/.chap/model";
// const CHAP_VB_DIR: &'static str = "~/.chap/data";
// const CHAP_LOG_DIR: &'static str = "~/.chap/log";

// 单例的 Tokio runtime
pub(crate) static RUNTIME: Lazy<tokio::runtime::Runtime> = Lazy::new(|| {
    Builder::new_multi_thread()
        .enable_io()
        .enable_time() // Enable time (timers)
        .worker_threads(num_cpus::get())
        .build()
        .expect("Failed to create runtime")
});

//app
pub(crate) struct Chappie {
    tui: ChapTui,
    chap_log_dir: PathBuf,
    chap_plugin_dir: PathBuf,
}
//
impl Chappie {
    pub(crate) fn init(&self) -> ChapResult<()> {
        let home = dirs::home_dir().expect("Failed to get home directory");
        let chap_log_dir = home.join(".chap/log");
        let chap_plugin_dir = home.join(".chap/plugin");
        fs::create_dir_all(&chap_log_dir)?;
        fs::create_dir_all(&chap_plugin_dir)?;
        // 配置日志输出到文件
        WriteLogger::init(
            LevelFilter::Debug,                           // 设置日志级别
            Config::default(),                            // 使用默认日志配置
            File::create(chap_log_dir.join("chap.log"))?, // 创建日志文件
        )?;
        Ok(())
    }

    pub(crate) fn new(cli: &Cli) -> ChapResult<Chappie> {
        let (prompt_tx, prompt_rx) = mpsc::channel::<String>(1);
        let (llm_res_tx, llm_res_rx) = mpsc::channel::<String>(1);
        let home = dirs::home_dir().expect("Failed to get home directory");
        let chap_log_dir = home.join(".chap/log");
        let chap_plugin_dir = PathBuf::from("/home/postgres/rsproject/chappie/plugin"); //home.join(".chap/plugin");
        fs::create_dir_all(&chap_log_dir)?;
        fs::create_dir_all(&chap_plugin_dir)?;
        // 配置日志输出到文件
        WriteLogger::init(
            LevelFilter::Debug,                           // 设置日志级别
            Config::default(),                            // 使用默认日志配置
            File::create(chap_log_dir.join("chap.log"))?, // 创建日志文件
        )?;

        let chap_ui = ChapTui::new(
            cli.get_chap_mod(),
            prompt_tx,
            llm_res_rx,
            cli.get_ui_type(),
            cli.get_que(),
        )?;

        Ok(Self {
            tui: chap_ui,
            chap_log_dir: chap_log_dir,
            chap_plugin_dir: chap_plugin_dir,
        })
    }

    pub(crate) fn run<P: AsRef<Path>>(&mut self, p: P) -> ChapResult<()> {
        RUNTIME.block_on(async move { self.tui.render(p, self.chap_plugin_dir.as_path()).await })
    }
}

// async fn request_llm(
//     mut prompt_rx: mpsc::Receiver<String>,
//     llm_res_tx: mpsc::Sender<String>,
//     vdb: Option<Collection>,
//     // embed_model: Arc<TextEmbedding>,
//     schema: Schema,
//     mut llm_client: LlmClient,
// ) {
//     let field_id_prompt = schema.get_field("prompt").unwrap();
//     loop {
//         let field_id_answer = schema.get_field("answer").unwrap();
//         tokio::select! {
//            Some(prompt) = prompt_rx.recv() => {
//             match llm_client.request(&prompt).await {
//                 Ok(res)=>{
//                     // if let Some(vb) = &vdb {
//                     //     let embeddings = embed_model.embed(vec![&prompt], None).unwrap();
//                     //     let mut d = Document::new();
//                     //     d.add_text(field_id_prompt, &prompt);
//                     //     d.add_text(field_id_answer, &res);
//                     //     let v6 = Vector::from_slice(&embeddings[0], d).unwrap();
//                     //     // 插入向量数据库
//                     //     vb.add(v6).await.unwrap();
//                     // }
//                     llm_res_tx.send(res).await.unwrap();
//                 }
//                 Err(e)=>{
//                     llm_res_tx.send(e.to_string()).await.unwrap();
//                 }
//             }

//             }
//         }
//     }
// }
