use crate::pg::format_item_ids;
use crate::pg::format_va_extinfo;
use crate::pg::format_varatt_external;
use crate::pg::parse_heap_tuple_header;
use crate::pg::parse_pg_page_header;
use crate::plugin::Plugin;
use crate::ChapResult;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

pub(crate) struct FunctionPlugin {
    function_registry: HashMap<String, FunctionFn>,
}

impl FunctionPlugin {
    pub(crate) fn new() -> FunctionPlugin {
        let mut function_registry: HashMap<String, FunctionFn> = HashMap::new();
        function_registry.insert("hello".to_string(), hello);
        function_registry.insert("pg_page_header".to_string(), parse_pg_page_header);
        function_registry.insert("item_data".to_string(), format_item_ids);
        function_registry.insert("pg_heap_tuple".to_string(), parse_heap_tuple_header);
        function_registry.insert("format_va_extinfo".to_string(), format_va_extinfo);
        function_registry.insert("format_varatt_external".to_string(), format_varatt_external);
        FunctionPlugin {
            function_registry: function_registry,
        }
    }

    pub(crate) fn list_registered_functions(&self) -> Vec<String> {
        self.function_registry.keys().cloned().collect()
    }
}

impl Plugin for FunctionPlugin {
    fn eval(&self, name: &str, buf: &[u8]) -> ChapResult<String> {
        if let Some(func) = self.function_registry.get(name) {
            return Ok(func(buf));
        }
        Ok("no function call".to_string())
    }

    fn list(&self) -> ChapResult<String> {
        let mut names = self.list_registered_functions();
        if names.is_empty() {
            Ok("No functions registered.".to_string())
        } else {
            // 原地按字母升序排序
            names.sort();
            Ok(names
                .iter()
                .map(|name| format!("* {}", name))
                .collect::<Vec<_>>()
                .join("\n"))
        }
    }
}

// /// 打印注册表中所有已注册函数名
// pub(crate) fn list_registered_functions() -> Vec<String> {
//     let map = FUNCTION_REGISTRY.lock().unwrap();
//     map.keys().cloned().collect()
// }

// /// 格式化输出为字符串（每行一个函数名）
// pub(crate) fn format_function_list() -> String {
//     let mut names = list_registered_functions();
//     if names.is_empty() {
//         "No functions registered.".to_string()
//     } else {
//         // 原地按字母升序排序
//         names.sort();
//         names
//             .iter()
//             .map(|name| format!("* {}", name))
//             .collect::<Vec<_>>()
//             .join("\n")
//     }
// }

// 定义全局函数注册表
static FUNCTION_REGISTRY: Lazy<Mutex<HashMap<String, FunctionFn>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// 注册函数到全局 map
pub fn register(name: &str, f: FunctionFn) {
    let mut map = FUNCTION_REGISTRY.lock().unwrap();
    map.insert(name.to_string(), f);
}

// 检索函数（可选）
pub fn lookup(name: &str) -> Option<FunctionFn> {
    FUNCTION_REGISTRY.lock().unwrap().get(name).cloned()
}

// 示例函数实现
fn hello(buf: &[u8]) -> String {
    format!("hello")
}

type FunctionFn = fn(&[u8]) -> String;
