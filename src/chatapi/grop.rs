use crate::ChapResult;
use async_trait::async_trait;
use groq_api_rs::completion::client::CompletionOption;
use groq_api_rs::completion::{client::Groq, message::Message, request::builder};
use llmapi::*;

unsafe impl Send for GroqApi {}
unsafe impl Sync for GroqApi {}

#[register_llmapi]
pub(crate) struct GroqApi {
    groq: Groq,
    model: String,
}

#[async_trait]
impl LlmApi for GroqApi {
    fn new(api_key: &str, model: &str) -> Self
    where
        Self: Sized,
    {
        GroqApi {
            groq: Groq::new(api_key),
            model: model.to_string(),
        }
    }

    fn name() -> &'static str {
        "groq"
    }

    async fn request(&mut self, message: &str) -> ChapResult<String> {
        let message = Message::UserMessage {
            role: Some("user".to_string()),
            content: Some(message.to_string()),
            name: None,
            tool_call_id: None,
        };
        let request = builder::RequestBuilder::new(self.model.clone());
        self.groq.add_message(message);
        let res = self.groq.create(request).await?;
        match res {
            CompletionOption::NonStream(v) => return Ok(v.choices[0].message.content.to_string()),
            CompletionOption::Stream(v) => todo!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Command;
    use groq_api_rs::completion::client::CompletionOption;

    use super::*;
    use crate::util::mmap_file;
    use std::io;

    #[tokio::test]
    async fn test_grop() -> io::Result<()> {
        let messages = vec![Message::UserMessage {
            role: Some("user".to_string()),
            content: Some("Explain the importance of fast language models".to_string()),
            name: None,
            tool_call_id: None,
        }];
        let request = builder::RequestBuilder::new("mixtral-8x7b-32768".to_string());
        let api_key = "";

        let mut client = Groq::new(&api_key);
        client.add_messages(messages);

        let res = client.create(request).await;
        assert!(res.is_ok());
        let r = res.unwrap();
        match r {
            CompletionOption::NonStream(v) => {
                println!("xx:{:?}", v.choices[0].message.content)
            }
            CompletionOption::Stream(v) => {}
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_grop_stream() -> io::Result<()> {
        let messages = vec![Message::UserMessage {
            role: Some("user".to_string()),
            content: Some("Explain the importance of fast language models".to_string()),
            name: None,
            tool_call_id: None,
        }];
        let request =
            builder::RequestBuilder::new("mixtral-8x7b-32768".to_string()).with_stream(true);
        let api_key = "";

        let mut client = Groq::new(api_key);
        client.add_messages(messages);

        let res = client.create(request).await;
        let r = res.unwrap();
        match r {
            CompletionOption::NonStream(v) => {
                println!("xx:{:?}", v.choices[0].message.content)
            }
            CompletionOption::Stream(v) => {
                for x in v.iter() {
                    println!("xx:{:?}", x.choices[0].delta)
                }
            }
        }

        Ok(())
    }
}
