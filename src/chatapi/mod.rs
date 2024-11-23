use llmapi::*;
pub(crate) mod grop;
pub(crate) struct LlmClient {
    //llm_api: Box<dyn LlmApi>,
}

#[register_llmapi]
pub(crate) struct Example {}

impl LlmApi for Example {
    fn new(api_key: &str, model: &str) -> Self
    where
        Self: Sized,
    {
        Example {}
    }

    fn name() -> &'static str {
        "example"
    }

    fn req(&mut self, message: String) -> String {
        return "example".to_string();
    }
}

#[register_llmapi]
pub(crate) struct Example2 {}

impl LlmApi for Example2 {
    fn new(api_key: &str, model: &str) -> Self
    where
        Self: Sized,
    {
        Example2 {}
    }

    fn name() -> &'static str {
        "example2"
    }

    fn req(&mut self, message: String) -> String {
        return "Example2".to_string();
    }
}

#[register_llmapi]
pub(crate) struct Init1 {}

impl LlmApi for Init1 {
    fn new(api_key: &str, model: &str) -> Self
    where
        Self: Sized,
    {
        Init1 {}
    }

    fn name() -> &'static str {
        "Init1"
    }
    fn req(&mut self, message: String) -> String {
        return "req Init1".to_string();
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use llmapi::get_llmapi;
    #[test]
    fn test_llm() {
        // let file_path = "/root/start_vpn.sh";
        // let mmap = map_file(file_path)?;
        // let (navi, visible_content, length) = get_visible_content(&mmap, 0, 30, 5, "");
        // println!("{},{}", visible_content, length);
        // Ok(())
        register("example", |apikey, model| Box::new(Example {}));

        let mut r = get_llmapi("example", "", "");
        println!("{}", r.req("".to_string()))
    }

    #[test]
    fn test_reg_llm() {
        // let file_path = "/root/start_vpn.sh";
        // let mmap = map_file(file_path)?;
        // let (navi, visible_content, length) = get_visible_content(&mmap, 0, 30, 5, "");
        // println!("{},{}", visible_content, length);
        // Ok(())
        println!("xx");
        let r = Init1 {};
    }
}
