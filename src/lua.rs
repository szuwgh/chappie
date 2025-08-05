use crate::{error::ChapResult, plugin::Plugin};
use anyhow::Ok;
use mlua::prelude::*;
use std::path::Path;
use std::path::PathBuf;
pub(crate) struct LuaPlugin {
    lua: Lua,
    plugin_path: PathBuf,
}

impl LuaPlugin {
    fn new<P: AsRef<Path>>(plugin_path: P) -> LuaPlugin {
        Self {
            lua: Lua::new(),
            plugin_path: plugin_path.as_ref().to_path_buf(),
        }
    }
}

impl Plugin for LuaPlugin {
    fn eval(&self, name: &str, buf: &[u8]) -> String {}
    fn list(&self) -> String {}
}

fn lua_vec() -> ChapResult<()> {
    let data: Vec<u8> = b"hello world".to_vec();
    let lua = Lua::new();
    let lua_table = lua.create_table_from(data.iter().enumerate().map(|(i, &b)| (i + 1, b)))?;
    lua.globals().set("lua_table", lua_table)?;
    let script = r#"
        local str = ""
        for _, byte in ipairs(lua_table) do
            str = str .. string.char(byte)
        end
        return str
    "#;
    let result: String = lua.load(script).eval()?;
    println!("Lua 返回的字符串是: {}", result);
    Ok(())
}

fn lua_hello() -> ChapResult<()> {
    // 创建一个新的 Lua 实例
    let lua = Lua::new();

    // 执行后，获取返回值
    let result: i64 = // 执行一段简单的 Lua 脚本
    lua.load(
        r#"
        print("Hello from Lua!")
        local sum = 3 + 4
        return sum
    "#,
    )
    .eval()?;
    println!("Lua 返回的结果是: {}", result);

    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_lua_hello() {
        lua_vec().unwrap();
    }
}
