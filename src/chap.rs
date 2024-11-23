use crate::chatapi::LlmClient;
use crate::ChapTui;
use vectorbase::collection::Collection;

//app
struct Chappie {
    tui: ChapTui,
    vb: Collection,
    llm_cli: LlmClient,
}

impl Chappie {
    // 运行
    pub(crate) async fn run() {}
}
