use std::fs::File;
use std::fs::OpenOptions;
const MAGIC: &[u8; 4] = b"CHPU";
const VERSION: u8 = 1;
const HEADER_SIZE: u64 = 16;

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
    pub(crate) op: OpType,
    pub(crate) cursor_y: u32,    // undo 后光标恢复到的行
    pub(crate) cursor_x: u32,    // undo 后光标恢复到的列
    pub(crate) line_index: u32,  // GapBuffer 行索引
    pub(crate) byte_offset: u32, // 行内字节偏移
    pub(crate) data: Vec<u8>,
}

pub struct UndoFile {
    file: File,
    head: u64,
}

impl UndoFile {
    pub(crate) fn open() {
        // let path = path.as_ref();
        // if path.exists() {
        //     let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        //     let head = Self::read_head_from_file(&mut file)?;
        //     Ok(Self { file, head })
        // } else {
        //     let mut file = OpenOptions::new()
        //         .read(true)
        //         .write(true)
        //         .create(true)
        //         .open(path)?;
        //     let head = HEADER_SIZE;
        //     Self::write_header_to_file(&mut file, head)?;
        //     Ok(Self { file, head })
        // }
    }
}
