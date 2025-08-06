use crate::ChapResult;
pub(crate) trait Plugin {
    fn eval(&self, name: &str, buf: &[u8]) -> ChapResult<String>;
    fn list(&self) -> ChapResult<String>;
}
