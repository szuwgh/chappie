use crate::byteutil::ByteView;
use crate::pg::format_item_ids;
use crate::pg::format_va_extinfo;
use crate::pg::format_varatt_external;
use crate::pg::parse_heap_tuple_header;
use crate::pg::parse_pg_page_header;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, PartialEq)]
pub(crate) struct Function(pub(crate) String);

impl Function {
    pub(crate) fn call(&self, b: ByteView) -> String {
        // 从全局 registry 查找函数
        if let Some(func) = FUNCTION_REGISTRY.lock().unwrap().get(&self.0) {
            return func(&b);
        }
        "no function call".to_string()
    }
}

/// 打印注册表中所有已注册函数名
pub(crate) fn list_registered_functions() -> Vec<String> {
    let map = FUNCTION_REGISTRY.lock().unwrap();
    map.keys().cloned().collect()
}

/// 格式化输出为字符串（每行一个函数名）
pub(crate) fn format_function_list() -> String {
    let mut names = list_registered_functions();
    if names.is_empty() {
        "No functions registered.".to_string()
    } else {
        // 原地按字母升序排序
        names.sort();
        names
            .iter()
            .map(|name| format!("* {}", name))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

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
fn hello(b: &ByteView) -> String {
    format!("hello")
}

// 初始化（可在模块初始化时注册）
pub fn init_function_registry() {
    register("hello", hello);
    register("pg_page_header", parse_pg_page_header);
    register("item_data", format_item_ids);
    register("pg_heap_tuple", parse_heap_tuple_header);
    register("format_va_extinfo", format_va_extinfo);
    register("format_varatt_external", format_varatt_external);
}

type FunctionFn = fn(&ByteView) -> String;
