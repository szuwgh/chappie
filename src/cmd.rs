use clap::Parser;
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub(crate) struct Cli {
    /// 输入文件
    #[arg(short = 'f', long)]
    filepath: String,

    /// 要处理的名字
    #[arg(short = 'l', long, default_value = "groq")]
    llm: String,

    /// 要处理的名字
    #[arg(short = 'm', long, default_value = "llama3-8b-8192")]
    model: String,

    /// 要处理的名字
    #[arg(short = 'k', long)]
    api_key: String,
}

impl Cli {
    pub(crate) fn get_filepath(&self) -> &str {
        &self.filepath
    }
    pub(crate) fn get_llm(&self) -> &str {
        &self.llm
    }

    pub(crate) fn get_model(&self) -> &str {
        &self.model
    }

    pub(crate) fn get_api_key(&self) -> &str {
        &self.api_key
    }
}
