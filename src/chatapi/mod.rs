pub(crate) mod gemini;
pub(crate) mod grop;

use anyhow::Ok;
use async_trait::async_trait;
use llmapi::*;

use crate::error::{ChapError, ChapResult};

pub(crate) struct LlmClient {
    llm_api: Box<dyn LlmApi>,
}

impl LlmClient {
    pub(crate) fn new(_t: &str, api_key: &str, model: &str) -> ChapResult<LlmClient> {
        let llm_api = llmapi::get_llmapi(_t, api_key, model)
            .ok_or(ChapError::LLMNotRegistered(_t.to_string()))?;
        Ok(LlmClient { llm_api: llm_api })
    }

    pub async fn request(&mut self, message: &str) -> ChapResult<String> {
        self.llm_api.request(message).await
    }
}

#[register_llmapi]
pub(crate) struct EmptyLLM {}

#[async_trait]
impl LlmApi for EmptyLLM {
    fn new(api_key: &str, model: &str) -> Self
    where
        Self: Sized,
    {
        EmptyLLM {}
    }

    fn name() -> &'static str {
        "empty"
    }

    async fn request(&mut self, message: &str) -> ChapResult<String> {
        return Ok("empty llm no answer".to_string());
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use llmapi::get_llmapi;
    #[test]
    fn test_llm() {
        // // let file_path = "/root/start_vpn.sh";
        // // let mmap = map_file(file_path)?;
        // // let (navi, visible_content, length) = get_visible_content(&mmap, 0, 30, 5, "");
        // // println!("{},{}", visible_content, length);
        // // Ok(())
        // register("example", |apikey, model| Box::new(Example {}));

        // let mut r = get_llmapi("example", "", "");
        // // println!("{}", r.req("".to_string()))
    }
}
