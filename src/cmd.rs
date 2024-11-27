use crate::error::{ChapError, ChapResult};
use clap::Parser;
use clap::ValueEnum;
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub(crate) struct Cli {
    #[arg(value_name = "FILE")]
    filepath: Option<String>,

    #[arg(short = 'l', long, default_value = "groq", env = "CHAP_LLM_NAME")]
    llm: String,

    #[arg(short = 'm', long, env = "CHAP_LLM_MODEL")]
    model: Option<String>,

    #[arg(short = 'k', long, env = "CHAP_LLM_API_KEY")]
    api_key: String,

    #[arg(value_enum, long, default_value = "full", env = "CHAP_UI")]
    ui: UIType,

    #[arg(long = "vb", default_value_t = false, env = "CHAP_VB")]
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
        if let Some(p) = &self.model {
            return p;
        } else {
            return "";
        }
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
