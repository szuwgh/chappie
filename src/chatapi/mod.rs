pub(crate) mod grop;

trait LlmApi {
    async fn request(&mut self, message: String) -> String;
}

// struct LlmClient {
//     llm_api: Box<dyn LlmApi>,
// }
