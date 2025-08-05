use crate::byteutil::ByteView;
use std::fmt;
#[repr(C)]
#[derive(Debug)]
pub struct PageHeaderData {
    pub pd_lsn: u64,
    pub pd_checksum: u16,
    pub pd_flags: u16,
    pub pd_lower: u16,
    pub pd_upper: u16,
    pub pd_special: u16,
    pub pd_pagesize_version: u16,
    pub pd_prune_xid: u32,
}

impl PageHeaderData {
    /// 计算 item_count：页面中的行指针数量
    fn item_count(&self) -> usize {
        const HEADER_SIZE: usize = 24;
        // 如果 pd_lower 小于等于 header，说明没指针
        if (self.pd_lower as usize) <= HEADER_SIZE {
            0
        } else {
            ((self.pd_lower as usize) - HEADER_SIZE) / 4
        }
    }

    fn page_size(&self) -> usize {
        (self.pd_pagesize_version & 0xFF00) as usize
    }
    fn layout_version(&self) -> u8 {
        (self.pd_pagesize_version & 0x00FF) as u8
    }
}

impl fmt::Display for PageHeaderData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f,
            "PageHeaderData {{\n  pd_lsn:           0x{:016x}\n  checksum:         {}\n  flags:            0x{:04x}\n  lower:            {}\n  upper:            {}\n  special:          {}\n  pagesize:         {} bytes\n  layout_version:   {}\n  prune_xid:        {}\n  item_count:       {}\n}}",
            self.pd_lsn,
            self.pd_checksum,
            self.pd_flags,
            self.pd_lower,
            self.pd_upper,
            self.pd_special,
            self.page_size(),
            self.layout_version(),
            self.pd_prune_xid,
            self.item_count(),
        )
    }
}

pub(crate) fn parse_pg_page_header(b: &ByteView) -> String {
    let buf = b.get_data();
    if buf.len() < 8 + 2 * 6 + 4 {
        return "pg_page_header < 24".to_string();
    }
    let pd_lsn = u64::from_le_bytes(buf[0..8].try_into().unwrap());
    let pd_checksum = u16::from_le_bytes(buf[8..10].try_into().unwrap());
    let pd_flags = u16::from_le_bytes(buf[10..12].try_into().unwrap());
    let pd_lower = u16::from_le_bytes(buf[12..14].try_into().unwrap());
    let pd_upper = u16::from_le_bytes(buf[14..16].try_into().unwrap());
    let pd_special = u16::from_le_bytes(buf[16..18].try_into().unwrap());
    let pd_pagesize_version = u16::from_le_bytes(buf[18..20].try_into().unwrap());
    let pd_prune_xid = u32::from_le_bytes(buf[20..24].try_into().unwrap());
    let p = PageHeaderData {
        pd_lsn,
        pd_checksum,
        pd_flags,
        pd_lower,
        pd_upper,
        pd_special,
        pd_pagesize_version,
        pd_prune_xid,
    };
    p.to_string()
}

#[derive(Debug)]
pub struct ItemIdData {
    pub lp_off: u16,  // 行数据相对于页面起始的偏移（15 位）
    pub lp_flags: u8, // 标志（2 位）
    pub lp_len: u16,  // tuple 长度（15 位）
}

impl std::fmt::Display for ItemIdData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let flags_desc = match self.lp_flags {
            0 => "LP_UNUSED",
            1 => "LP_NORMAL",
            2 => "LP_REDIRECT",
            3 => "LP_DEAD",
            _ => "UNKNOWN",
        };
        write!(
            f,
            "ItemIdData {{ off: {}, len: {}, flags: {} ({}) }}",
            self.lp_off, self.lp_len, self.lp_flags, flags_desc
        )
    }
}

/// 从 page header 中已解析的 pd_lower 计算 item count（指针数量）
/// 并逐个读取每个行指针，解析 bit 字段。
pub fn format_item_ids(b: &ByteView) -> String {
    let buf = b.get_data();
    if buf.len() < 4 {
        return "item_ids < 4".to_string();
    }
    let raw = u32::from_le_bytes(buf[..4].try_into().unwrap());
    let lp_off = (raw & 0x7FFF) as u16;
    let lp_flags = ((raw >> 15) & 0x3) as u8;
    let lp_len = ((raw >> 17) & 0x7FFF) as u16;
    let item = ItemIdData {
        lp_off,
        lp_flags,
        lp_len,
    };
    item.to_string()
}

#[derive(Debug)]
pub struct HeapTupleHeader {
    pub xmin: u32,
    pub xmax: u32,
    pub cid_or_xvac: u32,
    pub t_ctid_block: u32,
    pub t_ctid_index: u16,
    pub infomask2: u16,
    pub infomask: u16,
    pub t_hoff: u8,
    pub has_nulls: bool,
    pub null_bitmap_bytes: usize,
}

impl fmt::Display for HeapTupleHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "HeapTupleHeader {{")?;
        writeln!(f, "  xmin: {}", self.xmin)?;
        writeln!(f, "  xmax: {}", self.xmax)?;
        writeln!(f, "  cid_or_xvac: {}", self.cid_or_xvac)?;
        writeln!(
            f,
            "  ctid: (block {}, index {})",
            self.t_ctid_block, self.t_ctid_index
        )?;
        writeln!(f, "  infomask2: 0x{:04x}", self.infomask2)?;
        // 解析 infomask2
        let natts = self.infomask2 & 0x07ff;
        let hot = (self.infomask2 & 0x4000) != 0;
        let heap_only = (self.infomask2 & 0x8000) != 0;
        writeln!(f, "    → natts (列数): {}", natts)?;
        writeln!(f, "    → HOT_UPDATED flag: {}", hot)?;
        writeln!(f, "    → HEAP_ONLY_TUPLE flag: {}", heap_only)?;
        writeln!(f, "  infomask:  0x{:04x}", self.infomask)?;
        // 解析 infomask 标志
        let mut flags = Vec::new();
        macro_rules! chk {
            ($mask:expr, $name:expr) => {
                if self.infomask & $mask != 0 {
                    flags.push($name);
                }
            };
        };
        chk!(0x0001, "HASNULL");
        chk!(0x0002, "HASVARWIDTH");
        chk!(0x0004, "HASEXTERNAL");
        chk!(0x0008, "HASOID");
        chk!(0x0100, "XMIN_COMMITTED");
        chk!(0x0200, "XMIN_INVALID");
        chk!(0x0400, "XMAX_COMMITTED");
        chk!(0x0800, "XMAX_INVALID");
        chk!(0x2000, "UPDATED");
        // …根据需要可添加更多宏表位…
        writeln!(f, "    → flags: {:?}", flags)?;
        writeln!(f, "  t_hoff: {} bytes", self.t_hoff)?;
        writeln!(f, "  has_nulls: {}", self.has_nulls)?;
        writeln!(f, "  null_bitmap_bytes: {}", self.null_bitmap_bytes)?;
        write!(f, "}}")
    }
}

pub(crate) fn parse_heap_tuple_header(b: &ByteView) -> String {
    let buf = b.get_data();
    // 需要至少到 t_hoff 字节：4+4+4+6+2+2+1 = 23 字节
    if buf.len() < 23 {
        return "heap_tuple_header < 23".to_string();
    }
    let xmin = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    let xmax = u32::from_le_bytes(buf[4..8].try_into().unwrap());
    let cid_or_xvac = u32::from_le_bytes(buf[8..12].try_into().unwrap());

    // t_ctid: ItemPointerData 包含 block 和 index（6 字节）
    let block = u32::from_le_bytes(buf[12..16].try_into().unwrap());
    let index = u16::from_le_bytes(buf[16..18].try_into().unwrap());

    let infomask2 = u16::from_le_bytes(buf[18..20].try_into().unwrap());
    let infomask = u16::from_le_bytes(buf[20..22].try_into().unwrap());
    let t_hoff = buf[22];

    // 判断是否 has nulls
    let has_nulls = (infomask & 0x0001) != 0;
    // null bitmap 长度：有多少列(attribute count)，由 infomask2 的低 11 位给出
    let natts = infomask2 & 0x07ff;
    let null_bitmap_bytes = if has_nulls {
        // 位图按位对齐到字节，位数 = natts
        ((natts as usize + 7) / 8)
    } else {
        0
    };

    HeapTupleHeader {
        xmin,
        xmax,
        cid_or_xvac,
        t_ctid_block: block,
        t_ctid_index: index,
        infomask2,
        infomask,
        t_hoff,
        has_nulls,
        null_bitmap_bytes,
    }
    .to_string()
}

/// varatt_external 结构在 tuple 中保存的位置，请你确保 buf 是从 va_data 开始读取的
pub fn format_va_extinfo(b: &ByteView) -> String {
    let buf = b.get_data();
    // 确保至少能读取 va_extinfo （4 bytes）在 offset 4，从 va_rawsize 后
    if buf.len() < 4 {
        return "buf < 4".to_string();
    }
    // va_rawsize = first 4 bytes, little endian
    //    let va_rawsize = i32::from_le_bytes(buf[0..4].try_into().unwrap());
    // va_extinfo = next 4 bytes
    let extinfo = u32::from_le_bytes(buf[..4].try_into().unwrap());
    const MASK: u32 = (1u32 << 30) - 1; // VARLENA_EXTSIZE_MASK
    let extsize = extinfo & MASK;
    let method = extinfo >> 30;
    let comp = match method {
        0 => "PGLZ",
        1 => "LZ4",
        _ => "unknown",
    };
    format!(
        "va_extinfo raw = 0x{:08X}\nextsize = {}\ncompression method = {} (id={})",
        extinfo, extsize, comp, method
    )
}

#[derive(Debug)]
pub struct VarattExternal {
    va_rawsize: i32,
    va_extinfo: u32,
    va_extsize: u32,
    compression_method: &'static str,
    va_valueid: u32,
    va_toastrelid: u32,
}

impl fmt::Display for VarattExternal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "va_rawsize = {}\n\
             va_extinfo = 0x{:08X}\n\
             va_extsize = {}\n\
             compression method = {} (id={})\n\
             va_valueid (chunk_id) = {}\n\
             va_toastrelid = {}",
            self.va_rawsize,
            self.va_extinfo,
            self.va_extsize,
            self.compression_method,
            (self.va_extinfo >> 30),
            self.va_valueid,
            self.va_toastrelid
        )
    }
}

/// 解析一个 varlena pointer datum buffer，返回解析后的 `VarattExternal`
pub fn format_varatt_external(b: &ByteView) -> String {
    // 需要至少 header(1B) + tag(1B) + struct size (16B)
    let attr = b.get_data();
    const VA_EXTERNAL_SIZE: usize = 16;
    const HEADER_AND_TAG: usize = 2;
    if attr.len() < HEADER_AND_TAG + VA_EXTERNAL_SIZE {
        return "buf < 18".to_string();
    }
    let header = attr[0];
    let tag = attr[1];
    // 判断是否为 varattrib_1b_e
    if header != 0x01 || tag != 18 {
        return "is varattrib_1b_e".to_string();
    }
    // 从 byte 偏移 2 开始是 varatt_external
    let start = HEADER_AND_TAG;
    let rawsize = i32::from_le_bytes(attr[start..start + 4].try_into().unwrap());
    let extinfo = u32::from_le_bytes(attr[start + 4..start + 8].try_into().unwrap());
    let valueid = u32::from_le_bytes(attr[start + 8..start + 12].try_into().unwrap());
    let toastrelid = u32::from_le_bytes(attr[start + 12..start + 16].try_into().unwrap());

    const MASK: u32 = (1u32 << 30) - 1;
    let extsize = extinfo & MASK;
    let method_id = extinfo >> 30;
    let compression_method = match method_id {
        0 => "PGLZ",
        1 => "LZ4",
        _ => "UNKNOWN",
    };

    VarattExternal {
        va_rawsize: rawsize,
        va_extinfo: extinfo,
        va_extsize: extsize,
        compression_method,
        va_valueid: valueid,
        va_toastrelid: toastrelid,
    }
    .to_string()
}
