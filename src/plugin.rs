pub(crate) trait Plugin {
    fn eval(&self, name: &str, buf: &[u8]) -> String;
    fn list(&self) -> String;
}
