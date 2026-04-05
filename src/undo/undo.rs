use byteorder::ReadBytesExt;

use crate::common::error::ChapResult;
use byteorder::LittleEndian;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Cursor;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::Path;

const MAGIC: &[u8; 4] = b"CHPU";
const VERSION: u8 = 1;
const HEADER_SIZE: usize = 16;

// ── 操作类型 ──────────────────────────────────────────────────────────────────

#[repr(u8)]
#[derive(Debug, Clone, PartialEq)]
pub enum OpType {
    InsertChar = 1,
    DeleteChar = 2,
    InsertNewline = 3,
    DeleteNewline = 4,
}

impl OpType {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::InsertChar),
            2 => Some(Self::DeleteChar),
            3 => Some(Self::InsertNewline),
            4 => Some(Self::DeleteNewline),
            _ => None,
        }
    }

    /// 返回当前操作的逆操作类型
    fn inverse(&self) -> Self {
        match self {
            Self::InsertChar => Self::DeleteChar,
            Self::DeleteChar => Self::InsertChar,
            Self::InsertNewline => Self::DeleteNewline,
            Self::DeleteNewline => Self::InsertNewline,
        }
    }
}

pub(crate) struct EditOp {
    pub(crate) op_type: OpType,
    pub(crate) cursor_y: u32,    // undo 后光标恢复到的行
    pub(crate) cursor_x: u32,    // undo 后光标恢复到的列
    pub(crate) block_id: u32,    // 操作所在块的 block_id（split 后不变，跨块编辑也稳定）
    pub(crate) line_index: u32,  // GapBuffer 行索引（GapText 模式使用）
    pub(crate) byte_offset: u32, // 操作字节在块内的绝对偏移（操作完成后的位置，用于 undo backspace）
    pub(crate) data: Vec<u8>,
}

const RECORD_HEADER_SIZE: usize = 23; // op_type(1) + cursor_y(4) + cursor_x(4) + block_id(4) + line_index(4) + byte_offset(4) + data_len(2)

pub(crate) struct Record {
    pub(crate) op_type: OpType,
    pub(crate) cursor_y: u32,    // undo 后光标恢复到的行
    pub(crate) cursor_x: u32,    // undo 后光标恢复到的列
    pub(crate) block_id: u32,    // 操作所在块的 block_id
    pub(crate) line_index: u32,  // GapBuffer 行索引
    pub(crate) byte_offset: u32, // 块内绝对字节偏移（操作后位置）
    pub(crate) data: Vec<u8>,
    pub(crate) record_size: u16,
}

impl Record {
    fn to_edit_op(self) -> EditOp {
        EditOp {
            op_type: self.op_type,
            cursor_y: self.cursor_y,
            cursor_x: self.cursor_x,
            block_id: self.block_id,
            line_index: self.line_index,
            byte_offset: self.byte_offset,
            data: self.data,
        }
    }

    fn serialize<W: Write>(self, mut w: W) -> ChapResult<()> {
        let mut buf = Vec::with_capacity(self.record_size as usize);
        buf.push(self.op_type.clone() as u8);
        buf.extend_from_slice(&self.cursor_y.to_le_bytes());
        buf.extend_from_slice(&self.cursor_x.to_le_bytes());
        buf.extend_from_slice(&self.block_id.to_le_bytes());
        buf.extend_from_slice(&self.line_index.to_le_bytes());
        buf.extend_from_slice(&self.byte_offset.to_le_bytes());
        buf.extend_from_slice(&(self.data.len() as u16).to_le_bytes());
        buf.extend_from_slice(&self.data);
        buf.extend_from_slice(&self.record_size.to_le_bytes());
        w.write_all(&buf)?;
        Ok(())
    }

    fn deserialize<R: Read + Seek>(mut r: R) -> ChapResult<Record> {
        let mut fixed = [0u8; RECORD_HEADER_SIZE];
        r.read_exact(&mut fixed)?;
        let op_type = OpType::from_u8(fixed[0]).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "unknown op_type")
        })?;
        let mut c = Cursor::new(&fixed[1..]);
        let cursor_y = c.read_u32::<LittleEndian>()?;
        let cursor_x = c.read_u32::<LittleEndian>()?;
        let block_id = c.read_u32::<LittleEndian>()?;
        let line_index = c.read_u32::<LittleEndian>()?;
        let byte_offset = c.read_u32::<LittleEndian>()?;
        let data_len = c.read_u16::<LittleEndian>()? as usize;
        let mut data = vec![0u8; data_len];
        r.read_exact(&mut data)?;
        r.seek(SeekFrom::Current(2))?;
        Ok(Record {
            op_type,
            cursor_y,
            cursor_x,
            block_id,
            line_index,
            byte_offset,
            data,
            record_size: 0,
        })
    }
}

impl EditOp {
    /// 生成逆操作（op_type 翻转，其余字段不变）
    fn to_inverse(mut self) -> EditOp {
        self.op_type = self.op_type.inverse();
        self
    }
}

pub struct UndoFile {
    file: File,
    head: usize,
}

impl UndoFile {
    pub(crate) fn open<P: AsRef<Path>>(path: P) -> ChapResult<Self> {
        let path = path.as_ref();
        if path.exists() {
            let mut file = OpenOptions::new().read(true).write(true).open(path)?;
            let head = Self::read_head_from_file(&mut file)?;
            Ok(Self { file, head })
        } else {
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(path)?;
            let head = HEADER_SIZE;
            Self::write_header_to_file(&mut file, head)?;
            Ok(Self { file, head })
        }
    }

    fn write_record(file: &mut File, op: EditOp) -> ChapResult<u16> {
        let data_len = op.data.len() as u16;
        let record_size: u16 = RECORD_HEADER_SIZE as u16 + data_len + 2;
        let record = Record {
            op_type: op.op_type,
            cursor_y: op.cursor_y,
            cursor_x: op.cursor_x,
            block_id: op.block_id,
            line_index: op.line_index,
            byte_offset: op.byte_offset,
            data: op.data,
            record_size,
        };
        record.serialize(file)?;
        Ok(record_size)
    }

    pub(crate) fn push(&mut self, op: EditOp) -> ChapResult<()> {
        let inverse = op.to_inverse();
        self.file.set_len(self.head as u64)?;
        self.file.seek(SeekFrom::Start(self.head as u64))?;
        let record_size = Self::write_record(&mut self.file, inverse)?;
        self.head += record_size as usize;
        Self::write_header_to_file(&mut self.file, self.head)?;
        Ok(())
    }

    pub(crate) fn undo(&mut self) -> ChapResult<Option<EditOp>> {
        if self.head <= HEADER_SIZE {
            return Ok(None);
        }
        self.file.seek(SeekFrom::Start((self.head - 2) as u64))?;
        let record_size = self.file.read_u16::<LittleEndian>()?;
        if record_size < RECORD_HEADER_SIZE as u16 || self.head < record_size as usize {
            return Ok(None);
        }
        let record_start = self.head - record_size as usize;
        self.file.seek(SeekFrom::Start(record_start as u64))?;
        let record = Record::deserialize(&mut self.file)?;
        self.head = record_start;
        Self::write_header_to_file(&mut self.file, self.head)?;
        Ok(Some(record.to_edit_op()))
    }

    fn read_head_from_file(file: &mut File) -> std::io::Result<usize> {
        let mut buf = [0u8; HEADER_SIZE];
        file.seek(SeekFrom::Start(0))?;
        file.read_exact(&mut buf)?;
        if &buf[0..4] != MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid undo file magic",
            ));
        }
        Ok(usize::from_le_bytes(buf[8..16].try_into().unwrap()))
    }

    fn write_header_to_file(file: &mut File, head: usize) -> std::io::Result<()> {
        let mut buf = [0u8; HEADER_SIZE];
        buf[0..4].copy_from_slice(MAGIC);
        buf[4] = VERSION;
        buf[8..16].copy_from_slice(&head.to_le_bytes());
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn op(op_type: OpType, cy: u32, cx: u32, li: u32, bo: u32, data: Vec<u8>) -> EditOp {
        EditOp {
            op_type,
            cursor_y: cy,
            cursor_x: cx,
            block_id: 0,
            line_index: li,
            byte_offset: bo,
            data,
        }
    }

    // ── OpType ────────────────────────────────────────────────────────────────

    #[test]
    fn op_type_inverse_insert_char() {
        assert_eq!(OpType::InsertChar.inverse(), OpType::DeleteChar);
    }

    #[test]
    fn op_type_inverse_delete_char() {
        assert_eq!(OpType::DeleteChar.inverse(), OpType::InsertChar);
    }

    #[test]
    fn op_type_inverse_insert_newline() {
        assert_eq!(OpType::InsertNewline.inverse(), OpType::DeleteNewline);
    }

    #[test]
    fn op_type_inverse_delete_newline() {
        assert_eq!(OpType::DeleteNewline.inverse(), OpType::InsertNewline);
    }

    #[test]
    fn op_type_from_u8_all_valid() {
        assert_eq!(OpType::from_u8(1), Some(OpType::InsertChar));
        assert_eq!(OpType::from_u8(2), Some(OpType::DeleteChar));
        assert_eq!(OpType::from_u8(3), Some(OpType::InsertNewline));
        assert_eq!(OpType::from_u8(4), Some(OpType::DeleteNewline));
    }

    #[test]
    fn op_type_from_u8_invalid_returns_none() {
        assert!(OpType::from_u8(0).is_none());
        assert!(OpType::from_u8(5).is_none());
        assert!(OpType::from_u8(255).is_none());
    }

    // ── open ──────────────────────────────────────────────────────────────────

    #[test]
    fn open_creates_file_with_valid_magic() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        let _uf = UndoFile::open(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], MAGIC);
        assert_eq!(bytes[4], VERSION);
    }

    #[test]
    fn open_new_file_head_equals_header_size() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        let uf = UndoFile::open(&path).unwrap();
        assert_eq!(uf.head, HEADER_SIZE);
    }

    #[test]
    fn open_existing_file_reads_head() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        {
            let mut uf = UndoFile::open(&path).unwrap();
            uf.push(op(OpType::InsertChar, 0, 0, 0, 0, vec![])).unwrap();
        }
        let uf2 = UndoFile::open(&path).unwrap();
        assert!(uf2.head > HEADER_SIZE);
    }

    #[test]
    fn open_invalid_magic_returns_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        let mut buf = [0u8; HEADER_SIZE];
        buf[0..4].copy_from_slice(b"XXXX");
        buf[8..16].copy_from_slice(&(HEADER_SIZE).to_le_bytes());
        std::fs::write(&path, &buf).unwrap();
        assert!(UndoFile::open(&path).is_err());
    }

    // ── undo empty ────────────────────────────────────────────────────────────

    #[test]
    fn undo_on_empty_file_returns_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        let mut uf = UndoFile::open(&path).unwrap();
        assert!(uf.undo().unwrap().is_none());
    }

    // ── push stores inverse op_type ───────────────────────────────────────────

    #[test]
    fn push_insert_char_undo_returns_delete_char() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        let mut uf = UndoFile::open(&path).unwrap();
        uf.push(op(OpType::InsertChar, 0, 0, 0, 0, vec![])).unwrap();
        let got = uf.undo().unwrap().unwrap();
        assert_eq!(got.op_type, OpType::DeleteChar);
    }

    #[test]
    fn push_delete_char_undo_returns_insert_char() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        let mut uf = UndoFile::open(&path).unwrap();
        uf.push(op(OpType::DeleteChar, 0, 0, 0, 0, vec![])).unwrap();
        let got = uf.undo().unwrap().unwrap();
        assert_eq!(got.op_type, OpType::InsertChar);
    }

    #[test]
    fn push_insert_newline_undo_returns_delete_newline() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        let mut uf = UndoFile::open(&path).unwrap();
        uf.push(op(OpType::InsertNewline, 0, 0, 0, 0, vec![]))
            .unwrap();
        let got = uf.undo().unwrap().unwrap();
        assert_eq!(got.op_type, OpType::DeleteNewline);
    }

    #[test]
    fn push_delete_newline_undo_returns_insert_newline() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        let mut uf = UndoFile::open(&path).unwrap();
        uf.push(op(OpType::DeleteNewline, 0, 0, 0, 0, vec![]))
            .unwrap();
        let got = uf.undo().unwrap().unwrap();
        assert_eq!(got.op_type, OpType::InsertNewline);
    }

    // ── field preservation ────────────────────────────────────────────────────

    #[test]
    fn push_undo_preserves_all_numeric_fields() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        let mut uf = UndoFile::open(&path).unwrap();
        uf.push(op(OpType::InsertChar, 42, 7, 3, 100, vec![]))
            .unwrap();
        let got = uf.undo().unwrap().unwrap();
        assert_eq!(got.cursor_y, 42);
        assert_eq!(got.cursor_x, 7);
        assert_eq!(got.line_index, 3);
        assert_eq!(got.byte_offset, 100);
    }

    #[test]
    fn push_undo_preserves_empty_data() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        let mut uf = UndoFile::open(&path).unwrap();
        uf.push(op(OpType::InsertChar, 0, 0, 0, 0, vec![])).unwrap();
        let got = uf.undo().unwrap().unwrap();
        assert!(got.data.is_empty());
    }

    #[test]
    fn push_undo_preserves_single_byte_data() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        let mut uf = UndoFile::open(&path).unwrap();
        uf.push(op(OpType::DeleteChar, 5, 10, 0, 8, vec![b'Z']))
            .unwrap();
        let got = uf.undo().unwrap().unwrap();
        assert_eq!(got.data, vec![b'Z']);
    }

    #[test]
    fn push_undo_preserves_multibyte_utf8_data() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        let mut uf = UndoFile::open(&path).unwrap();
        let data = b"\xe4\xb8\xad".to_vec(); // '中' UTF-8
        uf.push(op(OpType::InsertChar, 1, 2, 3, 4, data.clone()))
            .unwrap();
        let got = uf.undo().unwrap().unwrap();
        assert_eq!(got.data, data);
    }

    // ── LIFO order ────────────────────────────────────────────────────────────

    #[test]
    fn multiple_push_undo_lifo_order() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        let mut uf = UndoFile::open(&path).unwrap();
        uf.push(op(OpType::InsertChar, 1, 0, 0, 0, vec![b'a']))
            .unwrap();
        uf.push(op(OpType::InsertChar, 2, 0, 0, 0, vec![b'b']))
            .unwrap();
        uf.push(op(OpType::InsertChar, 3, 0, 0, 0, vec![b'c']))
            .unwrap();

        let r3 = uf.undo().unwrap().unwrap();
        assert_eq!(r3.cursor_y, 3);
        assert_eq!(r3.data, vec![b'c']);

        let r2 = uf.undo().unwrap().unwrap();
        assert_eq!(r2.cursor_y, 2);
        assert_eq!(r2.data, vec![b'b']);

        let r1 = uf.undo().unwrap().unwrap();
        assert_eq!(r1.cursor_y, 1);
        assert_eq!(r1.data, vec![b'a']);

        assert!(uf.undo().unwrap().is_none());
    }

    #[test]
    fn undo_beyond_history_keeps_returning_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        let mut uf = UndoFile::open(&path).unwrap();
        uf.push(op(OpType::InsertChar, 0, 0, 0, 0, vec![])).unwrap();
        uf.undo().unwrap();
        assert!(uf.undo().unwrap().is_none());
        assert!(uf.undo().unwrap().is_none()); // 再次 undo 仍是 None
    }

    // ── push after undo truncates redo ────────────────────────────────────────

    #[test]
    fn push_after_undo_truncates_redo_history() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        let mut uf = UndoFile::open(&path).unwrap();
        uf.push(op(OpType::InsertChar, 1, 0, 0, 0, vec![b'a']))
            .unwrap();
        uf.push(op(OpType::InsertChar, 2, 0, 0, 0, vec![b'b']))
            .unwrap();
        uf.undo().unwrap(); // undo op2，head 退回

        // push op3：截断 op2 的 redo 历史
        uf.push(op(OpType::InsertChar, 3, 0, 0, 0, vec![b'c']))
            .unwrap();

        let r3 = uf.undo().unwrap().unwrap(); // 应该是 op3，而非 op2
        assert_eq!(r3.cursor_y, 3);
        assert_eq!(r3.data, vec![b'c']);

        let r1 = uf.undo().unwrap().unwrap(); // 再往前是 op1
        assert_eq!(r1.cursor_y, 1);

        assert!(uf.undo().unwrap().is_none()); // 栈空
    }

    // ── head tracking ─────────────────────────────────────────────────────────

    #[test]
    fn head_advances_after_push() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        let mut uf = UndoFile::open(&path).unwrap();
        let before = uf.head;
        uf.push(op(OpType::InsertChar, 0, 0, 0, 0, vec![])).unwrap();
        assert!(uf.head > before);
    }

    #[test]
    fn head_returns_to_header_size_after_full_undo() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        let mut uf = UndoFile::open(&path).unwrap();
        uf.push(op(OpType::InsertChar, 0, 0, 0, 0, vec![])).unwrap();
        uf.undo().unwrap();
        assert_eq!(uf.head, HEADER_SIZE);
    }

    // ── persistence across reopen ─────────────────────────────────────────────

    #[test]
    fn reopen_after_push_can_undo() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        {
            let mut uf = UndoFile::open(&path).unwrap();
            uf.push(op(OpType::InsertChar, 5, 3, 1, 10, vec![b'x']))
                .unwrap();
        }
        let mut uf2 = UndoFile::open(&path).unwrap();
        let got = uf2.undo().unwrap().unwrap();
        assert_eq!(got.op_type, OpType::DeleteChar);
        assert_eq!(got.cursor_y, 5);
        assert_eq!(got.data, vec![b'x']);
    }

    #[test]
    fn reopen_after_undo_head_reflects_current_position() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("undo.log");
        {
            let mut uf = UndoFile::open(&path).unwrap();
            uf.push(op(OpType::InsertChar, 0, 0, 0, 0, vec![])).unwrap();
            uf.undo().unwrap(); // head 退回 HEADER_SIZE，写入文件
        }
        let uf2 = UndoFile::open(&path).unwrap();
        assert_eq!(uf2.head, HEADER_SIZE);
    }
}
