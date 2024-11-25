use crate::error::{ChapError, ChapResult};
use clap::Parser;
use clap::ValueEnum;
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub(crate) struct Cli {
    #[arg(value_name = "FILE")]
    filepath: Option<String>,

    #[arg(short = 'l', long, default_value = "groq", env = "LLM_NAME")]
    llm: String,

    #[arg(short = 'm', long, default_value = "llama3-8b-8192", env = "LLM_MODEL")]
    model: String,

    #[arg(short = 'k', long, env = "LLM_API_KEY")]
    api_key: String,

    #[arg(value_enum, long, default_value = "full")]
    ui: UIType,

    #[arg(long = "vb", default_value_t = false)]
    vb: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, ValueEnum)]
pub(crate) enum UIType {
    Full,
    Lite,
}

impl Cli {
    pub(crate) fn get_filepath(&self) -> ChapResult<&str> {
        if let Some(p) = &self.filepath {
            return Ok(p);
        } else {
            return Err(ChapError::NoFilePath.into());
        }
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

    pub(crate) fn get_ui_type(&self) -> UIType {
        self.ui
    }

    pub(crate) fn get_vb(&self) -> bool {
        self.vb
    }
}
