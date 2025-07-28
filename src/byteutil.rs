use half::f16;
use std::ascii::escape_default;

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

    pub(crate) fn to_f32(&self) -> f32 {
        convert!(&self.data, f32, self.endian)
    }

    pub(crate) fn to_f64(&self) -> f64 {
        convert!(&self.data, f64, self.endian)
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

    pub(crate) fn to_varlena(&self) -> Option<(VarlenaType, u32)> {
        parse_varlena_header(&self.data)
    }
}

/// 解析 PostgreSQL varlena header，识别类型并返回长度 + payload 起始偏移
#[derive(Debug, PartialEq)]
enum VarlenaType {
    ShortInline,          // varattrib_1b
    ToastPointer,         // varattrib_1b_e
    FourByteUncompressed, // varattrib_4b uncompressed
    FourByteCompressed,   // varattrib_4b compressed
}

fn parse_varlena_header(data: &[u8]) -> Option<(VarlenaType, u32)> {
    if data.is_empty() {
        return None;
    }
    let hdr = data[0];
    if hdr & 0x01 == 1 {
        // 1-byte header
        if hdr == 0x01 {
            // TOAST Pointer
            // header = 1 byte + 1 tag byte (总共 2 字节)
            return Some((VarlenaType::ToastPointer, 2));
        } else {
            // short inline, high7 bits = payload length
            let payload_len = (hdr >> 1) as u32;
            let total_len = payload_len;
            return Some((VarlenaType::ShortInline, total_len));
        }
    } else {
        // 4-byte header 格式
        if data.len() < 4 {
            return None;
        }
        let hdr_le = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let total_len = hdr_le >> 2;
        let is_compressed = (hdr_le & (1 << 30)) != 0;
        let typ = if is_compressed {
            VarlenaType::FourByteCompressed
        } else {
            VarlenaType::FourByteUncompressed
        };
        return Some((typ, total_len));
    }
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
