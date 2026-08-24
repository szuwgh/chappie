use crate::textwarp::ChapResult;
use crate::textwarp::EditText;
use crate::textwarp::GapBuffer;
use crate::textwarp::LineData;
use crate::textwarp::LineState;
use crate::textwarp::LineStr;
use crate::textwarp::Text;
use crate::textwarp::TextIndex;
use crate::textwarp::TextSelect;
use std::fs;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

const CHAR_GAP_SIZE: usize = 128;

pub(crate) struct GapText {
    lines: Vec<GapBuffer>,            // 每128字节使用 GapBuffer 存储
    file_size: usize,                 // 文件大小
    page_offset_list: Vec<LineState>, // 分页偏移列表
}

impl GapText {
    pub(crate) fn from_file_path<P: AsRef<Path>>(filename: P) -> ChapResult<GapText> {
        let file = File::open(filename)?;
        let mut reader = BufReader::new(file);
        let mut buffer = Vec::new();
        let mut gap_buffers: Vec<GapBuffer> = Vec::new();

        while reader.read_until(b'\n', &mut buffer)? > 0 {
            if let Some(&b'\n') = buffer.last() {
                buffer.pop();
            }
            // 兼容 CRLF，去除末尾的 '\r'
            if let Some(&b'\r') = buffer.last() {
                buffer.pop();
            }

            let gap_buffer = GapBuffer::from_bytes(&buffer, CHAR_GAP_SIZE);
            gap_buffers.push(gap_buffer);
            buffer.clear(); // 清空缓冲区，准备读取下一行
        }

        Ok(GapText {
            lines: gap_buffers,
            file_size: 0,
            page_offset_list: Vec::new(),
        })
    }

    fn borrow_lines(&self) -> &Vec<GapBuffer> {
        &self.lines
    }

    fn borrow_lines_mut(&mut self) -> &mut Vec<GapBuffer> {
        &mut self.lines
    }

    fn get_iter(&mut self, line_index: usize) -> GapTextIter<'_> {
        GapTextIter::new(&mut self.lines, line_index)
    }

    fn get_iter_rev(&mut self, line_index: usize) -> GapTextIterRev<'_> {
        GapTextIterRev::new(&mut self.lines, line_index)
    }

    pub(crate) fn get_text_len(&self, index: usize) -> usize {
        if index >= self.lines.len() {
            return 0;
        }
        self.lines[index].text_len()
    }

    fn rename_backup<P1: AsRef<Path>, P2: AsRef<Path>>(
        filepath: P1,
        backup_name: P2,
    ) -> ChapResult<()> {
        fs::rename(backup_name, filepath)?;
        Ok(())
    }

    pub(crate) fn save_file<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()> {
        let backup_name = Self::get_backup_name(&filepath)?;
        self.make_backup(&backup_name)?;
        Self::rename_backup(&filepath, &backup_name)?;
        Ok(())
    }

    fn get_backup_name<P: AsRef<Path>>(filepath: P) -> ChapResult<PathBuf> {
        let mut path = filepath.as_ref().to_path_buf();
        if let Some(file_name) = path.file_name() {
            path.set_file_name(format!(".{}.{}", file_name.to_string_lossy(), "chap"));
        }
        Ok(path)
    }
}

impl TextIndex for GapText {
    fn get_page_offset(&self, line_num: usize) -> LineState {
        LineState::default()
    }

    fn set_page_offset(&mut self, page_num: usize, page_offset: LineState) {
        // GapText 不支持分页偏移
    }
}

impl Text for GapText {
    type LineItem<'a> = LineStr<'a>;
    fn get_file_size(&self) -> usize {
        self.file_size
    }

    fn text_from_sel(&self, sel: &TextSelect) -> Vec<u8> {
        todo!("Not implement text_from_sel for GapText");
    }

    fn has_pre_line(&self, meta: &LineState) -> bool {
        if meta.get_line_index() == 0 && meta.get_line_end() == 0 {
            return false;
        }
        true
    }

    fn has_next_line(&self, meta: &LineState) -> bool {
        let line_index = meta.get_line_index();
        let line_end = meta.get_line_end();
        if line_index == self.lines.len() - 1 && line_end == self.get_text_len(line_index) {
            return false;
        }
        true
    }

    fn get_next_line_state(&self, state: &LineState) -> Option<LineState> {
        None
    }

    fn get_pre_line_state(&mut self, state: &LineState, width: usize) -> Option<LineState> {
        None
    }

    fn get_line<'a>(&'a self, state: &LineState) -> Option<LineStr<'a>> {
        Some(LineStr {
            data: LineData::GapBytes(self.borrow_lines()[state.line_index].text(..)),
            block_id: state.block_num,
            block_offset: state.block_offset,
            line_file_start: state.line_file_start,
            line_file_end: state.line_file_end,
        })
    }

    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize {
        self.borrow_lines()[line_index].text_len()
    }

    fn iter<'a>(&'a mut self, line_state: &LineState) -> impl Iterator<Item = LineStr<'a>> {
        self.get_iter(line_state.line_index)
    }

    fn iter_rev<'a>(&'a mut self, line_state: &LineState) -> impl Iterator<Item = LineStr<'a>> {
        self.get_iter_rev(line_state.line_index)
    }

    fn iter_u8<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_file_start: usize,
    ) -> impl Iterator<Item = u8> {
        Vec::new().into_iter() // GapText 不支持 u8 迭代
    }

    fn search(&mut self, partten: &[u8], state: &LineState) -> ChapResult<Option<Vec<LineState>>> {
        Ok(None)
    }
}

impl EditText for GapText {
    fn rollback() -> ChapResult<()> {
        Ok(())
    }
    fn backspace(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        count: usize,
        line_meta: &LineState,
    ) -> ChapResult<Vec<u8>> {
        let (line_index, line_offset) = (
            line_meta.get_line_index(),
            line_meta.get_line_offset() + bytes_cursor,
        );
        if self.borrow_lines_mut()[line_index].text_len() == 0 && line_offset == 0 {
            //删除一行
            self.borrow_lines_mut().remove(line_index);
            return Ok(vec![]);
        }
        //表示当前行和前一行合并
        if line_offset == 0 {
            if line_index == 0 {
                return Ok(vec![]);
            }
            let (pre_lines, cur_lines) = self.borrow_lines_mut().split_at_mut(line_index);
            let pre_line = &mut pre_lines[line_index - 1];
            if pre_line.text_len() == 0 {
                self.borrow_lines_mut().remove(line_index - 1);
                return Ok(vec![]);
            } else {
                let cur_line = &mut cur_lines[0];
                let cur_line_txt = cur_line.text(..);
                pre_line.insert(pre_line.text_len(), cur_line_txt.left());
                pre_line.insert(pre_line.text_len(), cur_line_txt.right());
                self.borrow_lines_mut().remove(line_index);
            }
            return Ok(vec![]);
        }
        self.borrow_lines_mut()[line_index].backspace(line_offset, count);
        Ok(vec![])
    }

    fn insert_bytes(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        c: &[u8],
        is_overwrite: bool,
    ) -> ChapResult<()> {
        todo!()
    }

    fn insert_char(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        c: char,
    ) -> ChapResult<()> {
        let (line_index, line_offset) = (
            line_meta.get_line_index(),
            line_meta.get_line_offset() + bytes_cursor,
        );
        let mut buf = [0u8; 4]; // 一个 char 最多需要 4 个字节存储 UTF-8 编码
        let s: &str = c.encode_utf8(&mut buf);
        let line = &mut self.borrow_lines_mut()[line_index];
        line.insert(line_offset, s.as_bytes());
        Ok(())
    }

    fn insert_newline(
        &mut self,
        cursor_y: usize,
        cursor_x: usize,
        line_meta: &LineState,
    ) -> ChapResult<()> {
        let (line_index, line_offset) = (
            line_meta.get_line_index(),
            line_meta.get_line_offset() + cursor_x,
        );
        let line_txt = self.borrow_lines_mut()[line_index].text(..);
        let line_len = line_txt.len();
        {
            if line_offset > line_len {
                // 如果光标不在行尾，插入新行
                let new_gap_buffer = GapBuffer::new(10);
                self.borrow_lines_mut()
                    .insert(line_index + 1, new_gap_buffer);
            } else {
                let b = &line_txt.text(line_offset..);
                let mut new_gap_buffer = GapBuffer::new(b.len() + 5);
                new_gap_buffer.insert(0, b.left());
                new_gap_buffer.insert(new_gap_buffer.text_len(), b.left());
                self.borrow_lines_mut()
                    .insert(line_index + 1, new_gap_buffer);
            }
        }

        {
            if line_len > line_offset {
                let delete_len = line_len - line_offset;
                // 删除当前行的剩余部分
                self.borrow_lines_mut()[line_index].delete(line_len, delete_len);
            }
        }
        Ok(())
    }

    fn delete_line(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
    ) -> ChapResult<()> {
        Ok(())
    }

    fn make_backup<P: AsRef<Path>>(&mut self, backup_name: P) -> ChapResult<()> {
        // 备份文件
        let file = std::fs::File::create(backup_name).unwrap();
        let mut w = std::io::BufWriter::new(&file);
        for line in self.borrow_lines_mut().iter_mut() {
            let txt = line.text(..);
            w.write(txt.left()).unwrap();
            w.write(txt.right()).unwrap();
            w.write(b"\n").unwrap();
        }
        w.flush()?;
        Ok(())
    }

    fn ensure_block_loaded(&mut self, _: usize) -> ChapResult<()> {
        Ok(())
    }
}

struct GapTextIter<'a> {
    lines: std::slice::IterMut<'a, GapBuffer>,
}

impl<'a> GapTextIter<'a> {
    fn new(lines: &'a mut [GapBuffer], line_index: usize) -> GapTextIter<'a> {
        GapTextIter {
            lines: lines[line_index..].iter_mut(),
        }
    }
}

impl<'a> Iterator for GapTextIter<'a> {
    type Item = LineStr<'a>;
    fn next(&mut self) -> Option<LineStr<'a>> {
        self.lines.next().map(|line| LineStr {
            data: LineData::GapBytes(line.text(..)),
            block_id: 0,
            block_offset: 0,
            line_file_start: 0,
            line_file_end: 0,
        })
    }
}

struct GapTextIterRev<'a> {
    lines: std::iter::Rev<std::slice::IterMut<'a, GapBuffer>>,
}

impl<'a> GapTextIterRev<'a> {
    fn new(lines: &'a mut [GapBuffer], line_index: usize) -> GapTextIterRev<'a> {
        GapTextIterRev {
            lines: lines[..=line_index].iter_mut().rev(),
        }
    }
}

impl<'a> Iterator for GapTextIterRev<'a> {
    type Item = LineStr<'a>;
    fn next(&mut self) -> Option<LineStr<'a>> {
        self.lines.next().map(|line| LineStr {
            data: LineData::GapBytes(line.text(..)),
            block_id: 0,
            block_offset: 0,
            line_file_start: 0,
            line_file_end: 0,
        })
    }
}
