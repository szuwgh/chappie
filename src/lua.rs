use crate::{error::ChapResult, plugin::Plugin};
use mlua::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
pub(crate) struct LuaScript {
    desc: PathBuf,
    script: PathBuf,
}

pub(crate) struct LuaPlugin {
    lua: Lua,
    plugin_path: PathBuf,
    scripts_registry: HashMap<String, LuaScript>,
}

impl LuaPlugin {
    pub(crate) fn new<P: AsRef<Path>>(plugin_path: P) -> LuaPlugin {
        let mut scripts_registry: HashMap<String, LuaScript> = HashMap::new();

        // 读取plugin_path下的所有子目录
        if let Ok(entries) = fs::read_dir(&plugin_path) {
            for entry in entries {
                if let Ok(entry) = entry {
                    // 只处理子目录
                    if let Ok(file_type) = entry.file_type() {
                        if file_type.is_dir() {
                            let dir_name = entry.file_name();
                            let dir_name_str = dir_name.to_string_lossy().to_string();
                            // 构建desc.txt的路径
                            let desc_path = entry.path().join("desc.txt");
                            // 构建lua的路径
                            let expected_lua_name = dir_name_str.clone() + ".lua";
                            let script_path = entry.path().join(&expected_lua_name);
                            let lua_script = LuaScript {
                                desc: desc_path,
                                script: script_path,
                            };
                            scripts_registry.insert(dir_name_str, lua_script);
                        }
                    }
                }
            }
        }

        Self {
            lua: Lua::new(),
            plugin_path: plugin_path.as_ref().to_path_buf(),
            scripts_registry,
        }
    }

    fn eval_lua_script(&self, name: &str, buf: &[u8]) -> ChapResult<String> {
        let lua_scr = self
            .scripts_registry
            .get(name)
            .ok_or_else(|| format!("Script '{}' not found in registry", name))?;

        let lua_table = self
            .lua
            .create_table_from(buf.iter().enumerate().map(|(i, &b)| (i + 1, b)))?;
        self.lua.globals().set("lua_table", lua_table)?;
        let script_content = fs::read_to_string(lua_scr.script.as_path())
            .map_err(|e| format!("Failed to read Lua script: {}", e))?;
        let result: String = self.lua.load(script_content).eval()?;
        Ok(result)
    }

    fn list_registered(&self) -> Vec<String> {
        self.scripts_registry.keys().cloned().collect()
    }
}

impl Plugin for LuaPlugin {
    fn eval(&self, name: &str, buf: &[u8]) -> ChapResult<String> {
        self.eval_lua_script(name, buf)
    }
    fn list(&self) -> ChapResult<String> {
        let mut names = self.list_registered();
        if names.is_empty() {
            Ok("No lua registered.".to_string())
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
