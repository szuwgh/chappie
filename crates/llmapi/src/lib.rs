use async_trait::async_trait;
pub use ctor::ctor;
pub use llmapi_macro::register_llmapi;
use once_cell::sync::Lazy;
use std::collections::HashMap;

pub(crate) static mut LLM_REGISTRY: Lazy<
    HashMap<&'static str, Box<dyn (Fn(&str, &str) -> Box<dyn LlmApi>) + Sync + Send>>,
> = Lazy::new(|| HashMap::new());

pub fn register<F: Send + Sync + 'static>(_type: &'static str, f: F)
where
    F: Fn(&str, &str) -> Box<dyn LlmApi>,
{
    unsafe {
        LLM_REGISTRY.insert(_type, Box::new(f));
    }
}

pub fn get_llmapi(_t: &str, api_key: &str, model: &str) -> Option<Box<dyn LlmApi>> {
    unsafe {
        let r = LLM_REGISTRY.get(_t)?;
        Some(r(api_key, model))
    }
}

#[async_trait]
pub trait LlmApi: Send {
    fn new(api_key: &str, model: &str) -> Self
    where
        Self: Sized;

    fn name() -> &'static str
    where
        Self: Sized;

    async fn request(&mut self, message: &str) -> anyhow::Result<String>;
}
