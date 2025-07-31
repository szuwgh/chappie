use half::f16;
use std::ascii::escape_default;
use std::fmt::Display;
macro_rules! convert_be {
    ($data:expr, $t:ty) => {{
        const SZ: usize = std::mem::size_of::<$t>();
        let slice: &[u8] = $data;
        let mut arr = [0u8; SZ];
        if slice.len() >= SZ {
            arr.copy_from_slice(&slice[slice.len() - SZ..]);
        } else {
            arr[SZ - slice.len()..].copy_from_slice(slice);
        }
        <$t>::from_be_bytes(arr)
    }};
}

macro_rules! convert_le {
    ($data:expr, $t:ty) => {{
        const SZ: usize = std::mem::size_of::<$t>();
        let slice: &[u8] = $data;
        let mut arr = [0u8; SZ];
        let to_copy = slice.len().min(SZ);
        arr[..to_copy].copy_from_slice(&slice[..to_copy]);
        <$t>::from_le_bytes(arr)
    }};
}

macro_rules! convert {
    ($data:expr, $t:ty, $endian:expr) => {
        match $endian {
            Endian::Little => convert_le!($data, $t),
            Endian::Big => convert_be!($data, $t),
        }
    };
}
#[derive(Debug, PartialEq, Clone)]
pub(crate) enum Endian {
    Little,
    Big,
}

pub(crate) struct ByteView {
    data: Vec<u8>,
    endian: Endian,
}

impl ByteView {
    pub(crate) fn new(data: Vec<u8>, endian: Endian) -> Self {
        ByteView {
            data,
            endian: endian,
        }
    }

    pub(crate) fn get_data(&self) -> &[u8] {
        &self.data
    }

    pub(crate) fn to_binary_8bit(&self) -> String {
        //只转化第一个u8
        self.data
            .get(0)
            .map_or("00000000".to_string(), |&b| format!("{:08b}", b))
    }

    pub(crate) fn len(&self) -> usize {
        self.data.len()
    }

    pub(crate) fn to_u8(&self) -> u8 {
        *self.data.get(0).unwrap_or(&0)
    }

    pub(crate) fn to_u16(&self) -> u16 {
        convert!(&self.data, u16, self.endian)
    }

    pub(crate) fn to_i16(&self) -> i16 {
        convert!(&self.data, i16, self.endian)
    }

    pub(crate) fn to_u24(&self) -> u32 {
        convert!(&self.data, u32, self.endian)
    }

    pub(crate) fn to_i24(&self) -> i32 {
        ((self.to_u24() as i32) << 8) >> 8
    }

    pub(crate) fn to_u32(&self) -> u32 {
        convert!(&self.data, u32, self.endian)
    }

    pub(crate) fn to_i32(&self) -> i32 {
        convert!(&self.data, i32, self.endian)
    }

    pub(crate) fn to_u48(&self) -> u64 {
        let bytes = [
            *self.data.get(0).unwrap_or(&0),
            *self.data.get(1).unwrap_or(&0),
            *self.data.get(2).unwrap_or(&0),
            *self.data.get(3).unwrap_or(&0),
            *self.data.get(4).unwrap_or(&0),
            *self.data.get(5).unwrap_or(&0),
        ];
        u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], 0, 0,
        ])
    }

    pub(crate) fn to_u64(&self) -> u64 {
        convert!(&self.data, u64, self.endian)
    }

    pub(crate) fn to_i64(&self) -> i64 {
        convert!(&self.data, i64, self.endian)
    }

    pub(crate) fn to_f16(&self) -> f16 {
        convert!(&self.data, f16, self.endian)
    }

    pub(crate) fn to_f32(&self) -> SmartF32 {
        SmartF32(convert!(&self.data, f32, self.endian))
    }

    pub(crate) fn to_f64(&self) -> SmartF64 {
        SmartF64(convert!(&self.data, f64, self.endian))
    }

    pub(crate) fn to_str(&self) -> String {
        let mut s = String::new();
        for &b in &self.data {
            for byte in escape_default(b) {
                s.push(byte as char);
            }
        }
        s
    }

    pub(crate) fn to_varlena(&self) -> VarlenaData {
        parse_varlena_header(&self.data).unwrap_or(VarlenaData(VarlenaType::Unknown, 0))
    }
}

pub(crate) struct SmartF32(f32);

impl Display for SmartF32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 优先获取 formatter 的宽度参数
        let width = f.width().unwrap_or(40);

        // 使用默认格式生成字符串
        let s = format!("{}", self.0);

        let out = if s.len() > width {
            // 超过 width，使用科学计数法（LowerExp）
            if let Some(prec) = f.precision() {
                format!("{:.*e}", prec, self.0)
            } else {
                format!("{:e}", self.0)
            }
        } else {
            // 未超过宽度，直接用默认格式（或带 precision）
            if let Some(prec) = f.precision() {
                format!("{:.*}", prec, self.0)
            } else {
                s
            }
        };

        // 使用 pad 结合宽度和对齐方式输出
        f.pad(&out)
    }
}

/// 包装浮点数，提供条件科学计数法输出的 Display 实现。
pub(crate) struct SmartF64(f64);

impl Display for SmartF64 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 优先获取 formatter 的宽度参数
        let width = f.width().unwrap_or(40);

        // 使用默认格式生成字符串
        let s = format!("{}", self.0);

        let out = if s.len() > width {
            // 超过 width，使用科学计数法（LowerExp）
            if let Some(prec) = f.precision() {
                format!("{:.*e}", prec, self.0)
            } else {
                format!("{:e}", self.0)
            }
        } else {
            // 未超过宽度，直接用默认格式（或带 precision）
            if let Some(prec) = f.precision() {
                format!("{:.*}", prec, self.0)
            } else {
                s
            }
        };

        // 使用 pad 结合宽度和对齐方式输出
        f.pad(&out)
    }
}

impl Display for VarlenaData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 首先构造基础字符串
        let s = format!("{:?},{}", self.0, self.1);
        // 截断 precision
        let truncated = if let Some(max) = f.precision() {
            if s.len() > max {
                &s[..max]
            } else {
                &s
            }
        } else {
            &s[..]
        };
        // 使用 pad 添加对齐和填充
        f.pad(truncated)
    }
}

#[derive(Debug)]
pub(crate) struct VarlenaData(VarlenaType, u32);

/// 解析 PostgreSQL varlena header，识别类型并返回长度 + payload 起始偏移
#[derive(Debug, PartialEq)]
enum VarlenaType {
    ShortInline,          // varattrib_1b
    ToastPointer,         // varattrib_1b_e
    FourByteUncompressed, // varattrib_4b uncompressed
    FourByteCompressed,   // varattrib_4b compressed
    Unknown,
}

fn parse_varlena_header(data: &[u8]) -> Option<VarlenaData> {
    if data.is_empty() {
        return None;
    }
    let hdr = data[0];
    if hdr & 0x01 == 1 {
        // 1-byte header
        if hdr == 0x01 {
            // TOAST Pointer
            // header = 1 byte + 1 tag byte (总共 2 字节)
            return Some(VarlenaData(VarlenaType::ToastPointer, 2));
        } else {
            // short inline, high7 bits = payload length
            let payload_len = (hdr >> 1) as u32;
            let total_len = payload_len;
            return Some(VarlenaData(VarlenaType::ShortInline, total_len));
        }
    } else {
        // 4-byte header 格式
        if data.len() < 4 {
            return None;
        }
        let hdr_le = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let total_len = hdr_le >> 2;
        let is_compressed = (data[0] & 0x0010) != 0;
        let typ = if is_compressed {
            VarlenaType::FourByteCompressed
        } else {
            VarlenaType::FourByteUncompressed
        };
        return Some(VarlenaData(typ, total_len));
    }
}

pub fn format_va_extinfo(buf: &[u8]) -> String {
    // 确保至少能读取 va_extinfo （4 bytes）在 offset 4，从 va_rawsize 后
    if buf.len() < 8 {
        return "buf < 8".to_string();
    }
    // va_rawsize = first 4 bytes, little endian
    let va_rawsize = i32::from_le_bytes(buf[0..4].try_into().unwrap());
    // va_extinfo = next 4 bytes
    let extinfo = u32::from_le_bytes(buf[4..8].try_into().unwrap());
    const MASK: u32 = (1u32 << 30) - 1; // VARLENA_EXTSIZE_MASK
    let extsize = extinfo & MASK;
    let method = extinfo >> 30;
    let comp = match method {
        0 => "PGLZ",
        1 => "LZ4",
        _ => "unknown",
    };
    format!(
        "va_rawsize = {}\nva_extinfo raw = 0x{:08X}\nextsize = {}\ncompression method = {} (id={})",
        va_rawsize, extinfo, extsize, comp, method
    )
}

#[cfg(test)]
mod tests {
    use half::vec;

    use super::*;
    use crate::util::mmap_file;
    use std::io;

    #[test]
    fn test_to_u16() {
        let data = vec![0x34, 0x12];
        let bv = ByteView::new(data, Endian::Little);
        assert_eq!(bv.to_u16(), 0x1234);

        let data = vec![0x34, 0x12];
        let bv = ByteView::new(data, Endian::Big);
        assert_eq!(bv.to_u16(), 0x3412);
    }

    #[test]
    fn test_to_varlena() {
        let data = vec![0x19];
        let bv = ByteView::new(data, Endian::Little);
        println!("bv: {:?}", bv.to_varlena());
    }

    //  fn test
}
