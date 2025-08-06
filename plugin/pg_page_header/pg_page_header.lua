if #lua_table < 24 then
    return "pg_page_header < 24"
end

local buf_str = string.char(table.unpack(lua_table, 1, #lua_table))
-- 小端序解析二进制数据（使用 Lua 5.3+ string.unpack）
local pos = 1
local pd_lsn, pos = string.unpack("<I8", buf_str, pos)         -- 8字节 LSN
local pd_checksum, pos = string.unpack("<I2", buf_str, pos)   -- 2字节校验和
local pd_flags, pos = string.unpack("<I2", buf_str, pos)       -- 2字节标志位
local pd_lower, pos = string.unpack("<I2", buf_str, pos)       -- 2字节空闲空间起始
local pd_upper, pos = string.unpack("<I2", buf_str, pos)       -- 2字节空闲空间结束
local pd_special, pos = string.unpack("<I2", buf_str, pos)     -- 2字节特殊空间
local pd_pagesize_version, pos = string.unpack("<I2", buf_str, pos) -- 2字节页大小+版本
local pd_prune_xid, pos = string.unpack("<I4", buf_str, pos)   -- 4字节事务ID

-- 计算派生字段
local HEADER_SIZE = 24
local item_count = (pd_lower <= HEADER_SIZE) 
    and 0 
    or math.floor((pd_lower - HEADER_SIZE) / 4)  -- 行指针数量

local page_size = (pd_pagesize_version & 0xFF00)     -- 高8位：页大小
local layout_version = pd_pagesize_version & 0x00FF       -- 低8位：版本号

return string.format(
    [[PageHeaderData {
pd_lsn:           0x%016x
checksum:         %d
flags:            0x%04x
lower:            %d
upper:            %d
special:          %d
pagesize:         %d bytes
layout_version:   %d
prune_xid:        %d
item_count:       %d
}]],
    pd_lsn, pd_checksum, pd_flags, pd_lower,
    pd_upper, pd_special, page_size,
    layout_version, pd_prune_xid, item_count
)
