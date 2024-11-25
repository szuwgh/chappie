use crate::ChapResult;
use ask_gemini::Gemini;
use async_trait::async_trait;
use llmapi::*;

unsafe impl Send for GeminiApi {}
unsafe impl Sync for GeminiApi {}

#[register_llmapi]
struct GeminiApi {
    ge: Gemini,
}

#[async_trait]
impl LlmApi for GeminiApi {
    fn new(api_key: &str, model: &str) -> Self
    where
        Self: Sized,
    {
        GeminiApi {
            ge: Gemini::new(Some(api_key), Some(model)),
        }
    }

    fn name() -> &'static str {
        "gemini"
    }

    async fn request(&mut self, message: &str) -> ChapResult<String> {
        match self.ge.ask(message).await {
            Ok(response) => return Ok(response.join("\n")),
            Err(e) => return Err(e.into()),
        }
    }
}
