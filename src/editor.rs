use crate::mmap_file;
use crate::util::read_lines;
use crate::{error::ChapResult, gap_buffer::GapBuffer};
use inherit_methods_macro::inherit_methods;
use memmap2::Mmap;
use ratatui::symbols::line;
use std::cell::{RefCell, UnsafeCell};
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, Write};
use std::ops::RangeBounds;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use unicode_width::UnicodeWidthChar;
use utf8_iter::Utf8CharsEx;

const PAGE_GROUP: usize = 1;

const CHAR_GAP_SIZE: usize = 128;
const HEX_GAP_SIZE: usize = 4 * 1024;

pub(crate) enum TextType {
    Char, //字符
    Hex,  //16进制
}

#[derive(Debug, Default)]
pub(crate) struct EditLineMeta {
    //  txt: &'a str,
    txt_len: usize,         //文本长度
    char_len: usize,        //char字符大小
    page_num: usize,        //所在页数
    line_num: usize,        //行数
    line_index: usize,      //行号
    line_offset: usize,     //这一行在总行的偏移量位置
    line_file_start: usize, //行在文件开始位置
    line_file_end: usize,   //行在文件结束的位置
}

impl EditLineMeta {
    pub(crate) fn new(
        txt_len: usize,
        char_len: usize,
        page_num: usize,
        line_num: usize,
        line_index: usize,
        line_offset: usize,
        line_file_start: usize,
        line_file_end: usize,
    ) -> EditLineMeta {
        EditLineMeta {
            txt_len,
            char_len,
            page_num,
            line_num,
            line_index,
            line_offset,
            line_file_start: line_file_start,
            line_file_end: line_file_end,
        }
    }

    pub(crate) fn get_line_num(&self) -> usize {
        self.line_num
    }

    pub(crate) fn get_page_num(&self) -> usize {
        self.page_num
    }

    pub(crate) fn get_line_offset(&self) -> usize {
        self.line_offset
    }

    pub(crate) fn get_line_end(&self) -> usize {
        self.line_offset + self.txt_len
    }

    pub(crate) fn get_line_index(&self) -> usize {
        self.line_index
    }

    pub(crate) fn get_txt_len(&self) -> usize {
        self.txt_len
    }

    pub(crate) fn get_char_len(&self) -> usize {
        self.char_len
    }

    pub(crate) fn get_line_file_start(&self) -> usize {
        self.line_file_start
    }

    pub(crate) fn get_line_file_end(&self) -> usize {
        self.line_file_end
    }
}

pub(crate) struct RingVec<T> {
    cache: Vec<T>,
    start: usize,
    size: usize,
}

impl<T> RingVec<T> {
    pub(crate) fn new(size: usize) -> Self {
        RingVec {
            cache: Vec::with_capacity(size),
            start: 0,
            size: size,
        }
    }

    pub(crate) fn push_front(&mut self, item: T) {
        if self.cache.len() < self.size {
        } else {
            if self.start == 0 {
                self.start = self.size - 1;
            } else {
                self.start = (self.start - 1) % self.size;
            }
            self.cache[self.start] = item;
        }
    }

    pub(crate) fn push(&mut self, item: T) {
        if self.cache.len() < self.size {
            self.cache.push(item);
        } else {
            self.cache[self.start] = item;
            self.start = (self.start + 1) % self.size;
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.cache.len()
    }

    pub(crate) fn get(&self, index: usize) -> Option<&T> {
        if index >= self.cache.len() {
            return None;
        }
        let idx = (self.start + index) % self.cache.len();
        Some(&self.cache[idx])
    }

    pub(crate) fn last(&self) -> Option<&T> {
        if self.cache.is_empty() {
            return None;
        }
        let idx = (self.start + self.cache.len() - 1) % self.cache.len();
        Some(&self.cache[idx])
    }

    pub(crate) fn clear(&mut self) {
        self.cache.clear();
        self.start = 0;
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &T> {
        self.cache
            .iter()
            .cycle()
            .skip(self.start)
            .take(self.cache.len())
    }
}

pub(crate) struct CacheStr {
    data: NonNull<u8>,
    len: usize,
}

impl CacheStr {
    fn from_bytes(s: &[u8]) -> Self {
        let ptr = s.as_ptr() as *const u8 as *mut u8; // 获取 &str 的指针
        let len = s.len(); // 获取 &str 的长度
        let non_null_ptr = unsafe { NonNull::new_unchecked(ptr) }; // 创建 NonNull<str>
        CacheStr {
            data: non_null_ptr,
            len,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    // 从 CacheStr 获取 &str
    pub(crate) fn as_str(&self) -> &str {
        // 将指针转换为 &[u8]，然后转换为 &str
        let slice = unsafe { std::slice::from_raw_parts(self.data.as_ptr(), self.len) };
        std::str::from_utf8(slice).unwrap()
    }
}

pub(crate) trait Line {
    fn text_len(&self) -> usize;
    fn text(&mut self, range: impl RangeBounds<usize>) -> &[u8];
    fn text_str(&mut self, range: impl RangeBounds<usize>) -> &str;
}

pub(crate) trait Text {
    // type Item: Line;
    //是否有下一行
    fn has_next_line(&self, meta: &EditLineMeta) -> bool;

    fn get_line<'a>(
        &'a mut self,
        line_index: usize,
        line_start: usize,
        line_end: usize,
    ) -> LineStr<'a>;

    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize;

    fn iter<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_start: usize,
    ) -> impl Iterator<Item = LineStr<'a>>;
}

pub(crate) trait EditText {
    fn insert(&mut self, cursor_y: usize, cursor_x: usize, line_meta: &EditLineMeta, c: char);
    fn insert_newline(&mut self, cursor_y: usize, cursor_x: usize, line_meta: &EditLineMeta);
    fn backspace(&mut self, cursor_y: usize, cursor_x: usize, line_meta: &EditLineMeta);
    fn save<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()>;
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
        self.lines.next().map(|line| line.get_line_str())
    }
}

pub struct LineStr<'a> {
    pub(crate) line: &'a [u8],
    pub(crate) line_file_start: usize,
    pub(crate) line_file_end: usize,
}

impl<'a> LineStr<'a> {
    fn text_len(&self) -> usize {
        self.line.len()
    }

    fn text(&self, range: impl std::ops::RangeBounds<usize>) -> &'a [u8] {
        let start = match range.start_bound() {
            std::ops::Bound::Included(&start) => start,
            std::ops::Bound::Excluded(&start) => start + 1,
            std::ops::Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(&end) => end + 1,
            std::ops::Bound::Excluded(&end) => end,
            std::ops::Bound::Unbounded => self.text_len(),
        };
        //println!("start: {}, end: {} str:{}", start, end, self.line);
        &self.line[start..end]
    }
}

pub(crate) struct FileIoText {
    file: BufReader<File>,
}

impl FileIoText {
    pub(crate) fn from_file_path<P: AsRef<Path>>(filename: P) -> ChapResult<FileIoText> {
        let file = File::open(filename)?;
        Ok(FileIoText {
            file: BufReader::new(file),
        })
    }
}

pub struct FileIoTextIter<'a> {
    file: &'a BufReader<File>,
    line_index: usize,
    line_file_start: usize,
    line_file_end: usize,
}

// impl<'a> Iterator for FileIoTextIter<'a> {
//     type Item = LineStr<'a>;
//     fn next(&mut self) -> Option<LineStr<'a>> {
//         if self.line_file_start >= self.line_file_end {
//             return None;
//         }
//         self.file
//             .seek(std::io::SeekFrom::Current(self.line_file_start as i64))
//             .unwrap();
//         // let start = 0;
//         self.file.read_line(buf)
//         let line = &mmap[..end];
//         let line_start = self.line_file_start;
//         self.line_file_start += end + 1;
//         Some(LineStr {
//             line: std::str::from_utf8(line).unwrap(),
//             line_file_start: line_start,
//             line_file_end: line_start + end,
//         })
//     }
// }

pub(crate) struct HexText {
    buffer: GapBuffer,
}

impl HexText {
    pub(crate) fn from_file_path<P: AsRef<Path>>(filename: P) -> ChapResult<HexText> {
        let file = File::open(filename)?;
        //获取文件大小
        let file_size = file.metadata()?.len() as usize;

        let mut buffer = GapBuffer::new(file_size + HEX_GAP_SIZE);
        let mut reader = BufReader::new(file);
        let mut line = [0u8; 1024];
        while let Ok(n) = reader.read(&mut line) {
            if n == 0 {
                break;
            }
            println!("n:{}", n);
            buffer.insert(buffer.text_len(), &line[..n]);
        }
        Ok(HexText { buffer })
    }
}

impl Text for HexText {
    fn get_line<'a>(
        &'a mut self,
        line_index: usize,
        line_start: usize,
        line_end: usize,
    ) -> LineStr<'a> {
        todo!()
    }

    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize {
        line_end - line_start
    }

    fn has_next_line(&self, meta: &EditLineMeta) -> bool {
        todo!()
    }

    fn iter<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_start: usize,
    ) -> impl Iterator<Item = LineStr<'a>> {
        HexTextIter::new(&self.buffer.text(..), 8, line_start)
    }
}

struct HexTextIter<'a> {
    buffer: &'a [u8],
    with: usize,
    line_file_start: usize,
}

impl<'a> HexTextIter<'a> {
    fn new(buffer: &'a [u8], with: usize, line_file_start: usize) -> HexTextIter<'a> {
        HexTextIter {
            buffer,
            with,
            line_file_start: line_file_start,
        }
    }
}

impl<'a> Iterator for HexTextIter<'a> {
    type Item = LineStr<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.line_file_start >= self.buffer.len() {
            return None;
        }

        let buffer = &self.buffer[self.line_file_start..];
        let line_start = self.line_file_start;
        if self.with >= buffer.len() {
            self.line_file_start += buffer.len();
            Some(LineStr {
                line: buffer,
                line_file_start: line_start,
                line_file_end: buffer.len(),
            })
        } else {
            self.line_file_start += self.with;
            Some(LineStr {
                line: buffer[..self.with].as_ref(),
                line_file_start: line_start,
                line_file_end: line_start + self.with,
            })
        }
    }
}

pub(crate) struct MmapText {
    mmap: Mmap,
}

impl MmapText {
    pub(crate) fn from_file_path<P: AsRef<Path>>(filename: P) -> ChapResult<MmapText> {
        let mmap = mmap_file(filename)?;
        Ok(MmapText { mmap })
    }

    pub(crate) fn new(mmap: Mmap) -> MmapText {
        MmapText { mmap }
    }
}

pub struct MmapTextIter<'a> {
    mmap: &'a Mmap,
    line_index: usize,
    line_file_start: usize,
    line_file_end: usize,
}

impl<'a> Iterator for MmapTextIter<'a> {
    type Item = LineStr<'a>;
    fn next(&mut self) -> Option<LineStr<'a>> {
        if self.line_file_start >= self.line_file_end {
            return None;
        }
        let mmap = &self.mmap[self.line_file_start..];
        // let start = 0;
        let mut end = 0;
        for (i, byte) in mmap.iter().enumerate() {
            if *byte == b'\n' || i == mmap.len() - 1 {
                end = i;
                break;
            }
            end = i;
        }
        let line = &mmap[..end];
        let line_start = self.line_file_start;
        self.line_file_start += end + 1;
        Some(LineStr {
            line: line,
            line_file_start: line_start,
            line_file_end: line_start + end,
        })
    }
}

impl<'a> MmapTextIter<'a> {
    fn new(
        mmap: &'a Mmap,
        line_index: usize,
        line_file_start: usize,
        line_file_end: usize,
    ) -> MmapTextIter<'a> {
        MmapTextIter {
            mmap,
            line_index,
            line_file_start,
            line_file_end,
        }
    }
}

impl Text for MmapText {
    fn get_line<'a>(
        &'a mut self,
        line_index: usize,
        line_file_start: usize,
        line_file_end: usize,
    ) -> LineStr<'a> {
        // todo!()
        let line = &self.mmap[line_file_start..line_file_end];

        LineStr {
            line: line,
            line_file_start: line_file_start,
            line_file_end: line_file_end,
        }
    }

    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize {
        line_end - line_start
    }

    fn has_next_line(&self, meta: &EditLineMeta) -> bool {
        if meta.get_line_file_start() + meta.get_line_offset() + meta.get_txt_len()
            >= self.mmap.len() - 1
        {
            return false;
        }
        true
    }

    fn iter<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_start: usize,
    ) -> impl Iterator<Item = LineStr<'a>> {
        MmapTextIter::new(&self.mmap, line_index, line_start, self.mmap.len())
    }
}

pub(crate) struct GapText {
    lines: Vec<GapBuffer>, //每行使用 GapBuffer 存储
}

impl GapText {
    pub(crate) fn from_file_path<P: AsRef<Path>>(filename: P) -> ChapResult<GapText> {
        let lines = read_lines(filename)?;
        let mut gap_buffers: Vec<GapBuffer> = Vec::new();
        for line in lines {
            if let Ok(content) = line {
                let mut gap_buffer = GapBuffer::new(content.len() + CHAR_GAP_SIZE);
                gap_buffer.insert(0, content.as_bytes());
                gap_buffers.push(gap_buffer);
            }
        }

        Ok(GapText { lines: gap_buffers })
    }

    fn borrow_lines(&self) -> &Vec<GapBuffer> {
        &self.lines
    }

    fn borrow_lines_mut(&mut self) -> &mut Vec<GapBuffer> {
        &mut self.lines
    }

    fn get_iter(&mut self, line_index: usize) -> GapTextIter {
        GapTextIter::new(&mut self.lines, line_index)
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

    fn make_backup<P: AsRef<Path>>(&mut self, backup_name: P) -> ChapResult<()> {
        // 备份文件
        let file = std::fs::File::create(backup_name).unwrap();
        let mut w = std::io::BufWriter::new(&file);
        for line in self.borrow_lines_mut().iter_mut() {
            let txt = line.text(..);
            w.write(txt).unwrap();
            w.write(b"\n").unwrap();
        }
        w.flush()?;
        Ok(())
    }
}

impl Text for GapText {
    // type Item = LineStr<'a>;
    fn has_next_line(&self, meta: &EditLineMeta) -> bool {
        let mut line_index = meta.get_line_index();
        let mut line_end = meta.get_line_end();
        // 结束
        if line_index == self.lines.len() - 1 && line_end == self.get_text_len(line_index) {
            return false; // return (None, EditLineMeta::default());
        }
        true
    }

    fn get_line<'a>(
        &'a mut self,
        line_index: usize,
        line_start: usize,
        line_end: usize,
    ) -> LineStr<'a> {
        self.borrow_lines_mut()[line_index].get_line_str()
    }

    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize {
        self.borrow_lines()[line_index].text_len()
    }

    fn iter<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_start: usize,
    ) -> impl Iterator<Item = LineStr<'a>> {
        self.get_iter(line_index)
    }
}

impl EditText for GapText {
    fn backspace(&mut self, cursor_y: usize, cursor_x: usize, line_meta: &EditLineMeta) {
        let (line_index, line_offset) = (
            line_meta.get_line_index(),
            line_meta.get_line_offset() + cursor_x,
        );
        if self.borrow_lines_mut()[line_index].text_len() == 0 && line_offset == 0 {
            //删除一行
            self.borrow_lines_mut().remove(line_index);
            return;
        }
        //表示当前行和前一行合并
        if line_offset == 0 {
            if line_index == 0 {
                return;
            }
            //用.split_at_mut(position)修改代码
            let (pre_lines, cur_lines) = self.borrow_lines_mut().split_at_mut(line_index);
            let pre_line = &mut pre_lines[line_index - 1];
            if pre_line.text_len() == 0 {
                self.borrow_lines_mut().remove(line_index - 1);
                return;
            } else {
                let cur_line = &mut cur_lines[0];
                let cur_line_txt = cur_line.text(..);
                pre_line.insert(pre_line.text_len(), cur_line_txt);
                self.borrow_lines_mut().remove(line_index);
            }
            return;
        }
        self.borrow_lines_mut()[line_index].backspace(line_offset);
        // let page_offset_list = self.borrow_page_offset_list_mut();
        // unsafe { page_offset_list.set_len(line_meta.get_page_num()) };
        // self.borrow_cache_lines_mut().clear();
        // self.borrow_cache_line_meta_mut().clear();
    }

    fn insert(&mut self, cursor_y: usize, cursor_x: usize, line_meta: &EditLineMeta, c: char) {
        let (line_index, line_offset) = (
            line_meta.get_line_index(),
            line_meta.get_line_offset() + cursor_x,
        );

        let mut buf = [0u8; 4]; // 一个 char 最多需要 4 个字节存储 UTF-8 编码
        let s: &str = c.encode_utf8(&mut buf);
        let line = &mut self.borrow_lines_mut()[line_index];
        //如果line_offset大于文本长度 要填充空格
        if line_offset > line.text_len() {
            let gap_len = line_offset - line.text_len();
            line.insert(line.text_len(), " ".repeat(gap_len).as_bytes());
        }
        line.insert(line_offset, s.as_bytes());

        //切断page_offset_list 索引
        // let page_offset_list = self.borrow_page_offset_list_mut();
        // unsafe { page_offset_list.set_len(line_meta.get_page_num()) };
        // self.borrow_cache_lines_mut().clear();
        // self.borrow_cache_line_meta_mut().clear();
    }

    fn insert_newline(&mut self, cursor_y: usize, cursor_x: usize, line_meta: &EditLineMeta) {
        // let (line_index, line_offset) = self.calculate_x_y(cursor_y, cursor_x);
        let (line_index, line_offset) = (
            line_meta.get_line_index(),
            line_meta.get_line_offset() + cursor_x,
        );
        //    println!("line_index: {}, line_offset: {}", line_index, line_offset);
        let line_txt = self.borrow_lines_mut()[line_index].text(..);
        let line_len = line_txt.len();
        {
            if line_offset > line_len {
                // 如果光标不在行尾，插入新行
                let new_gap_buffer = GapBuffer::new(10);
                self.borrow_lines_mut()
                    .insert(line_index + 1, new_gap_buffer);
            } else {
                let b = &line_txt[line_offset..];
                let mut new_gap_buffer = GapBuffer::new(b.len() + 5);
                new_gap_buffer.insert(0, b);
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
        // let page_offset_list = self.borrow_page_offset_list_mut();
        // unsafe { page_offset_list.set_len(line_meta.get_page_num()) };
        // self.borrow_cache_lines_mut().clear();
        // self.borrow_cache_line_meta_mut().clear();
    }

    fn save<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()> {
        self.save_file(filepath)
    }
}

pub(crate) struct TextWarp<T: Text> {
    lines: UnsafeCell<T>,                               // 文本
    cache_lines: UnsafeCell<RingVec<CacheStr>>,         // 缓存行
    cache_line_meta: UnsafeCell<RingVec<EditLineMeta>>, // 缓存行
    page_offset_list: UnsafeCell<Vec<PageOffset>>,      // 每页的偏移量
    height: usize,                                      //最大行数
    with: usize,
    text_type: TextType,
}

impl<T: Text> TextWarp<T> {
    pub(crate) fn new(lines: T, height: usize, with: usize, text_type: TextType) -> TextWarp<T> {
        TextWarp {
            lines: UnsafeCell::new(lines),
            cache_lines: UnsafeCell::new(RingVec::new(height)),
            cache_line_meta: UnsafeCell::new(RingVec::new(height)),
            page_offset_list: UnsafeCell::new(vec![PageOffset {
                line_index: 0,
                line_offset: 0,
                line_file_start: 0,
            }]),
            height,
            with,
            text_type: text_type,
        }
    }

    fn borrow_lines(&self) -> &T {
        unsafe { &*self.lines.get() }
    }

    fn borrow_lines_mut(&self) -> &mut T {
        unsafe { &mut *self.lines.get() }
    }

    fn borrow_page_offset_list(&self) -> &Vec<PageOffset> {
        unsafe { &*self.page_offset_list.get() }
    }

    fn borrow_page_offset_list_mut(&self) -> &mut Vec<PageOffset> {
        unsafe { &mut *self.page_offset_list.get() }
    }

    fn borrow_cache_lines(&self) -> &RingVec<CacheStr> {
        unsafe { &*self.cache_lines.get() }
    }

    fn borrow_cache_lines_mut(&self) -> &mut RingVec<CacheStr> {
        unsafe { &mut *self.cache_lines.get() }
    }

    fn borrow_cache_line_meta(&self) -> &RingVec<EditLineMeta> {
        unsafe { &*self.cache_line_meta.get() }
    }

    fn borrow_cache_line_meta_mut(&self) -> &mut RingVec<EditLineMeta> {
        unsafe { &mut *self.cache_line_meta.get() }
    }

    pub(crate) fn get_text_len(&self, index: usize) -> usize {
        self.borrow_lines().get_line_text_len(index, 0, 0)
    }

    // 计算页码，等同于向上取整
    fn get_page_num(&self, num: usize) -> usize {
        (num + self.height - 1) / self.height
    }

    // 计算行数，等同于向上取整
    fn calculate_lines(text_len: usize, with: usize) -> usize {
        if text_len == 0 {
            return 1;
        }
        (text_len as f64 / with as f64).ceil() as usize
    }

    // 计算光标所在行和列
    // pub(crate) fn calculate_x_y(&self, cursor_y: usize, cursor_x: usize) -> (usize, usize) {
    //     let mut line_count = 0;
    //     let mut line_index = 0;
    //     let mut shirt = 0;
    //     // 计算光标所在行
    //     let y = cursor_y + 1;
    //     for (i, b) in self.borrow_lines().iter().enumerate() {
    //         let cur_line_count = Self::calculate_lines(b.text_len(), self.with);
    //         line_count += cur_line_count;
    //         line_index = i;
    //         if y <= line_count {
    //             shirt = cur_line_count - (line_count - y) - 1;
    //             break;
    //         }
    //     }
    //     let line_offset = shirt * self.with + cursor_x;
    //     (line_index, line_offset)
    // }

    //通过 line_index 获取页数
    // fn get_page_num_from_line_index(&self, line_index: usize) -> usize {
    //     for p in self.borrow_page_offset_list().iter() {
    //         if line_index < p.line_index {
    //             return p.line_index;
    //         }
    //     }
    // }

    //从当前行开始获取后面n行
    pub(crate) fn get_pre_line<'a>(
        &'a self,
        meta: &EditLineMeta,
        line_count: usize,
    ) -> (Option<CacheStr>, EditLineMeta) {
        if meta.get_line_num() == 1 {
            return (None, EditLineMeta::default());
        }
        let mut s: &[u8] = b"";
        let mut m = EditLineMeta::default();
        self.get_text(meta.get_line_num() - line_count, line_count, |txt, meta| {
            s = txt;
            m = meta;
        });
        (Some(CacheStr::from_bytes(s)), m)
    }

    //从当前行开始获取后面n行
    pub(crate) fn get_next_line<'a>(
        &'a self,
        meta: &EditLineMeta,
        line_count: usize,
    ) -> (Option<CacheStr>, EditLineMeta) {
        let mut line_index = meta.get_line_index();
        let mut line_end = meta.get_line_end();
        let mut line_file_start = meta.get_line_file_start();
        if !self.borrow_lines().has_next_line(meta) {
            return (None, EditLineMeta::default());
        }

        let line =
            self.borrow_lines_mut()
                .get_line(line_index, meta.line_file_start, meta.line_file_end); //&self.borrow_lines()[line_index];

        if line_end == line.text_len() {
            line_file_start = meta.get_line_file_end() + 1;
            line_end = 0;
            line_index += 1;
        }

        let p = PageOffset {
            line_index: line_index,
            line_offset: line_end,
            line_file_start: line_file_start,
        };
        let mut s: &[u8] = b"";
        let mut m = EditLineMeta::default();
        let start_page_num = meta.get_line_num() / self.height;
        self.get_char_text_fn(
            &p,
            line_count,
            meta.get_line_num(),
            start_page_num,
            0,
            &mut |x, m1| {
                s = x;
                m = m1;
            },
        );
        (Some(CacheStr::from_bytes(s)), m)
    }

    /**
     * 滚动下一行
     */
    pub(crate) fn scroll_next_one_line(
        &self,
        meta: &EditLineMeta,
    ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
        let (s, l) = self.get_next_line(meta, 1);
        if let Some(s) = s {
            self.borrow_cache_lines_mut().push(s);
            self.borrow_cache_line_meta_mut().push(l);
        }
        (self.borrow_cache_lines(), self.borrow_cache_line_meta())
    }

    /**
     * 滚动上一行
     */
    pub(crate) fn scroll_pre_one_line(
        &self,
        meta: &EditLineMeta,
    ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
        let (s, l) = self.get_pre_line(meta, 1);
        if let Some(s) = s {
            self.borrow_cache_lines_mut().push_front(s);
            self.borrow_cache_line_meta_mut().push_front(l);
        }
        (self.borrow_cache_lines(), self.borrow_cache_line_meta())
    }

    pub(crate) fn get_one_page(
        &self,
        line_num: usize,
    ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
        self.get_line_content(line_num, self.height)
    }

    pub(crate) fn get_current_page(&self) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
        (self.borrow_cache_lines(), self.borrow_cache_line_meta())
    }

    // 从第n行开始获取内容
    pub(crate) fn get_line_content(
        &self,
        line_num: usize,
        line_count: usize,
    ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
        self.borrow_cache_lines_mut().clear();
        self.borrow_cache_line_meta_mut().clear();

        self.get_text(line_num, line_count, |txt, meta| {
            self.borrow_cache_lines_mut()
                .push(CacheStr::from_bytes(txt));
            self.borrow_cache_line_meta_mut().push(meta);
        });

        (self.borrow_cache_lines(), self.borrow_cache_line_meta())
    }

    pub(crate) fn get_line_content_with_count(
        &self,
        line_num: usize,
        line_count: usize,
    ) -> (Vec<CacheStr>, Vec<EditLineMeta>) {
        let mut lines = Vec::new();
        let mut lines_meta = Vec::new();
        self.get_text(line_num, line_count, |txt, meta| {
            lines.push(CacheStr::from_bytes(txt));
            lines_meta.push(meta);
        });
        (lines, lines_meta)
    }

    fn get_text<'a, F>(&'a self, line_num: usize, line_count: usize, mut f: F)
    where
        F: FnMut(&'a [u8], EditLineMeta),
    {
        match self.text_type {
            TextType::Char => {
                assert!(line_num >= 1);
                // 计算页码
                let page_num = self.get_page_num(line_num);
                // 计算页码
                let mut index = (page_num - 1) / PAGE_GROUP;
                let page_offset_list = self.borrow_page_offset_list();
                let page_offset = if index >= page_offset_list.len() {
                    index = page_offset_list.len() - 1;
                    page_offset_list.last().unwrap()
                } else {
                    &page_offset_list[index]
                };
                let start_page_num = index * PAGE_GROUP;
                assert!(line_num >= start_page_num * self.height);
                //跳过的行数
                let skip_line = line_num;
                self.get_char_text_fn(
                    &page_offset,
                    line_count,
                    start_page_num * self.height,
                    start_page_num,
                    skip_line,
                    &mut f,
                );
            }
            TextType::Hex => {
                todo!()
            }
        }
    }

    fn get_hex_text_fn<'a, F>(
        &'a self,
        page_offset: &PageOffset,
        line_count: usize,
        start_line_num: usize,
        start_page_num: usize,
        skip_line: usize,
        f: &mut F,
    ) where
        // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
        F: FnMut(&'a [u8], EditLineMeta),
    {
        todo!()
    }

    fn get_char_text_fn<'a, F>(
        &'a self,
        page_offset: &PageOffset,
        line_count: usize,
        start_line_num: usize,
        start_page_num: usize,
        skip_line: usize,
        f: &mut F,
    ) where
        // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
        F: FnMut(&'a [u8], EditLineMeta),
    {
        // if page_offset.line_index >= self.borrow_lines().len() {
        //     return;
        // }
        // let (a, b) = self
        //     .borrow_lines_mut()
        //     .split_at_mut(page_offset.line_index + 1);
        let mut cur_line_count = 0;
        let mut line_num = start_line_num;
        let mut page_num = start_page_num;

        let iter = self.borrow_lines_mut().iter(
            page_offset.line_index,
            page_offset.line_offset,
            page_offset.line_file_start,
        );

        for (i, v) in iter.enumerate() {
            let line_offset = if i == 0 { page_offset.line_offset } else { 0 };
            Self::set_line_char_txt(
                v,
                page_offset.line_index + i,
                line_offset,
                self.with,
                self.height,
                self.borrow_page_offset_list_mut(),
                &mut line_num,
                line_count,
                &mut page_num,
                &mut cur_line_count,
                skip_line,
                f,
            );
            if cur_line_count >= line_count {
                return;
            }
        }

        // let line_txt = &a[page_offset.line_index].text()[page_offset.line_start..];
        // Self::set_line_txt(
        //     line_txt,
        //     page_offset.line_index,
        //     page_offset.line_start,
        //     self.with,
        //     self.height,
        //     self.borrow_page_offset_list_mut(),
        //     &mut line_num,
        //     line_count,
        //     &mut page_num,
        //     &mut cur_line_count,
        //     skip_line,
        //     f,
        // );
        // if cur_line_count >= line_count {
        //     return;
        // }
        // // 使用 split_at_mut 获取后续行的可变子切片
        // for (i, line) in b.iter_mut().enumerate() {
        //     let line_txt = line.text();
        //     Self::set_line_txt(
        //         line_txt,
        //         page_offset.line_index + i + 1,
        //         0,
        //         self.with,
        //         self.height,
        //         self.borrow_page_offset_list_mut(),
        //         &mut line_num,
        //         line_count,
        //         &mut page_num,
        //         &mut cur_line_count,
        //         skip_line,
        //         f,
        //     );
        //     if cur_line_count >= line_count {
        //         return;
        //     }
        // }
    }

    fn set_line_char_txt<'a, F>(
        line_str: LineStr<'a>,
        line_index: usize,
        line_start: usize,
        with: usize,
        height: usize,
        page_offset_list: &mut Vec<PageOffset>,
        line_num: &mut usize,
        line_count: usize,
        page_num: &mut usize,
        cur_line_count: &mut usize,
        skip_line: usize,
        f: &mut F,
    ) where
        // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
        F: FnMut(&'a [u8], EditLineMeta),
    {
        //空行
        let line_txt = line_str.text(line_start..);
        if line_txt.len() == 0 {
            *line_num += 1; //行数加1
            if *line_num >= skip_line {
                *cur_line_count += 1;
                f(
                    b"",
                    EditLineMeta::new(
                        0,
                        0,
                        *page_num + 1,
                        *line_num,
                        line_index,
                        0,
                        line_str.line_file_start,
                        line_str.line_file_end,
                    ),
                );
            }
            if *line_num % height == 0 {
                //到达一页
                *page_num += 1; //页数加1
                let m = *page_num / PAGE_GROUP;
                let n = *page_num % PAGE_GROUP;
                if n == 0 && m > page_offset_list.len() - 1 {
                    //保存页数的偏移量
                    page_offset_list.push(PageOffset {
                        line_index: line_index + 1,
                        line_offset: 0,
                        line_file_start: line_str.line_file_end,
                    });
                }
            }

            if *cur_line_count >= line_count {
                return;
            }
            return;
        }

        let mut current_width = 0; //
        let mut line_offset = 0; //
        let mut current_bytes = 0;
        let mut char_index = 0;
        let mut char_count = 0;

        for (i, (byte_index, ch)) in line_txt.char_indices().enumerate() {
            let ch_width = ch.width().unwrap_or(0);
            //检查是否超过屏幕宽度
            if current_width + ch_width > with {
                let end = (line_offset + current_bytes).min(line_txt.len());
                *line_num += 1; //行数加1
                if *line_num >= skip_line {
                    *cur_line_count += 1;
                    let txt = &line_txt[line_offset..end];
                    f(
                        txt,
                        EditLineMeta::new(
                            txt.len(),
                            i - char_index,
                            *page_num + 1,
                            *line_num,
                            line_index,
                            line_start + line_offset,
                            line_str.line_file_start,
                            line_str.line_file_end,
                        ),
                    );
                }
                if *line_num % height == 0 {
                    //到达一页
                    *page_num += 1; //页数加1
                    let m = *page_num / PAGE_GROUP;
                    let n = *page_num % PAGE_GROUP;
                    if n == 0 && m > page_offset_list.len() - 1 {
                        //保存页数的偏移量
                        page_offset_list.push(PageOffset {
                            line_index,
                            line_offset: line_start + byte_index,
                            line_file_start: line_str.line_file_start,
                        });
                    }
                }
                if *cur_line_count >= line_count {
                    return;
                }
                char_index = i;
                line_offset += current_bytes;
                current_width = 0;
                current_bytes = 0;
            }
            char_count += 1;
            current_width += ch_width;
            current_bytes += ch.len_utf8();
        }
        //当前行没有到达屏幕宽度 但还是一行
        if current_bytes > 0 {
            *line_num += 1;

            if *line_num >= skip_line {
                let txt = &line_txt[line_offset..];
                *cur_line_count += 1;
                f(
                    txt,
                    EditLineMeta::new(
                        txt.len(),
                        char_count - char_index,
                        *page_num + 1,
                        *line_num,
                        line_index,
                        line_start + line_offset,
                        line_str.line_file_start,
                        line_str.line_file_end,
                    ),
                );
            }
            if *line_num % height == 0 {
                *page_num += 1; //页数加1
                let m = *page_num / PAGE_GROUP;
                let n = *page_num % PAGE_GROUP;
                if n == 0 && m > page_offset_list.len() - 1 {
                    //保存页数的偏移量
                    page_offset_list.push(PageOffset {
                        line_index: line_index + 1,
                        line_offset: 0,
                        line_file_start: line_str.line_file_end,
                    });
                }
            }
            if *cur_line_count >= line_count {
                return;
            }
        }
    }
}

pub(crate) struct EditTextWarp<T: Text + EditText> {
    edit_text: TextWarp<T>,
}

#[inherit_methods(from = "self.edit_text")]
impl<T: Text + EditText> EditTextWarp<T> {
    pub(crate) fn new(
        lines: T,
        height: usize,
        with: usize,
        text_type: TextType,
    ) -> EditTextWarp<T> {
        EditTextWarp {
            edit_text: TextWarp::new(lines, height, with, text_type),
        }
    }

    pub(crate) fn get_one_page(
        &self,
        line_num: usize,
    ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>);

    pub(crate) fn get_current_page(&self) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>);

    /**
     * 滚动下一行
     */
    pub(crate) fn scroll_next_one_line(
        &self,
        meta: &EditLineMeta,
    ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>);

    /**
     * 滚动上一行
     */
    pub(crate) fn scroll_pre_one_line(
        &self,
        meta: &EditLineMeta,
    ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>);

    pub(crate) fn get_text_len(&self, index: usize) -> usize;

    // fn borrow_lines(&self) -> &T {
    //     unsafe { &*self.lines.get() }
    // }

    // fn borrow_lines_mut(&self) -> &mut T {
    //     unsafe { &mut *self.lines.get() }
    // }

    // fn borrow_page_offset_list(&self) -> &Vec<PageOffset> {
    //     unsafe { &*self.page_offset_list.get() }
    // }

    // fn borrow_page_offset_list_mut(&self) -> &mut Vec<PageOffset> {
    //     unsafe { &mut *self.page_offset_list.get() }
    // }

    // fn borrow_cache_lines(&self) -> &RingVec<CacheStr> {
    //     unsafe { &*self.cache_lines.get() }
    // }

    // fn borrow_cache_lines_mut(&self) -> &mut RingVec<CacheStr> {
    //     unsafe { &mut *self.cache_lines.get() }
    // }

    // fn borrow_cache_line_meta(&self) -> &RingVec<EditLineMeta> {
    //     unsafe { &*self.cache_line_meta.get() }
    // }

    // fn borrow_cache_line_meta_mut(&self) -> &mut RingVec<EditLineMeta> {
    //     unsafe { &mut *self.cache_line_meta.get() }
    // }

    // pub(crate) fn get_text_len(&self, index: usize) -> usize {
    //     self.borrow_lines().get_line_text_len(index)
    // }

    // // 计算页码，等同于向上取整
    // fn get_page_num(&self, num: usize) -> usize {
    //     (num + self.height - 1) / self.height
    // }

    // // 计算行数，等同于向上取整
    // fn calculate_lines(text_len: usize, with: usize) -> usize {
    //     if text_len == 0 {
    //         return 1;
    //     }
    //     (text_len as f64 / with as f64).ceil() as usize
    // }

    // 计算光标所在行和列
    // pub(crate) fn calculate_x_y(&self, cursor_y: usize, cursor_x: usize) -> (usize, usize) {
    //     let mut line_count = 0;
    //     let mut line_index = 0;
    //     let mut shirt = 0;
    //     // 计算光标所在行
    //     let y = cursor_y + 1;
    //     for (i, b) in self.borrow_lines().iter().enumerate() {
    //         let cur_line_count = Self::calculate_lines(b.text_len(), self.with);
    //         line_count += cur_line_count;
    //         line_index = i;
    //         if y <= line_count {
    //             shirt = cur_line_count - (line_count - y) - 1;
    //             break;
    //         }
    //     }
    //     let line_offset = shirt * self.with + cursor_x;
    //     (line_index, line_offset)
    // }

    //通过 line_index 获取页数
    // fn get_page_num_from_line_index(&self, line_index: usize) -> usize {
    //     for p in self.borrow_page_offset_list().iter() {
    //         if line_index < p.line_index {
    //             return p.line_index;
    //         }
    //     }
    // }

    // 插入字符
    // 计算光标所在行
    // 计算光标所在列
    pub(crate) fn insert(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        line_meta: &EditLineMeta,
        c: char,
    ) {
        self.edit_text
            .borrow_lines_mut()
            .insert(cursor_y, cursor_x, line_meta, c);
        //切断page_offset_list 索引
        let page_offset_list = self.edit_text.borrow_page_offset_list_mut();
        unsafe { page_offset_list.set_len(line_meta.get_page_num()) };
        self.edit_text.borrow_cache_lines_mut().clear();
        self.edit_text.borrow_cache_line_meta_mut().clear();
    }

    //插入换行
    pub(crate) fn insert_newline(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        line_meta: &EditLineMeta,
    ) {
        self.edit_text
            .borrow_lines_mut()
            .insert_newline(cursor_y, cursor_x, line_meta);
        let page_offset_list = self.edit_text.borrow_page_offset_list_mut();
        unsafe { page_offset_list.set_len(line_meta.get_page_num()) };
        self.edit_text.borrow_cache_lines_mut().clear();
        self.edit_text.borrow_cache_line_meta_mut().clear();
    }

    // 删除光标前一个字符
    pub(crate) fn backspace(&self, cursor_y: usize, cursor_x: usize, line_meta: &EditLineMeta) {
        self.edit_text
            .borrow_lines_mut()
            .backspace(cursor_y, cursor_x, line_meta);
        let page_offset_list = self.edit_text.borrow_page_offset_list_mut();
        unsafe { page_offset_list.set_len(line_meta.get_page_num()) };
        self.edit_text.borrow_cache_lines_mut().clear();
        self.edit_text.borrow_cache_line_meta_mut().clear();
    }

    pub(crate) fn save<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()> {
        self.edit_text.borrow_lines_mut().save(filepath)
    }

    // //从当前行开始获取后面n行
    // pub(crate) fn get_pre_line<'a>(
    //     &'a self,
    //     meta: &EditLineMeta,
    //     line_count: usize,
    // ) -> (Option<CacheStr>, EditLineMeta) {
    //     if meta.get_line_num() == 1 {
    //         return (None, EditLineMeta::default());
    //     }
    //     let mut s = "";
    //     let mut m = EditLineMeta::default();
    //     self.get_text(meta.get_line_num() - line_count, line_count, |txt, meta| {
    //         s = txt;
    //         m = meta;
    //     });
    //     (Some(CacheStr::from_str(s)), m)
    // }

    // //从当前行开始获取后面n行
    // pub(crate) fn get_next_line<'a>(
    //     &'a self,
    //     meta: &EditLineMeta,
    //     line_count: usize,
    // ) -> (Option<CacheStr>, EditLineMeta) {
    //     let mut line_index = meta.get_line_index();
    //     let mut line_end = meta.get_line_end();
    //     if !self.borrow_lines().has_next_line(meta) {
    //         return (None, EditLineMeta::default());
    //     }

    //     let line = self.borrow_lines().get_line(line_index); //&self.borrow_lines()[line_index];
    //     if line_end == line.text_len() {
    //         line_end = 0;
    //         line_index += 1;
    //     }

    //     let p = PageOffset {
    //         line_index: line_index,
    //         line_start: line_end,
    //     };
    //     let mut s = "";
    //     let mut m = EditLineMeta::default();
    //     let start_page_num = meta.get_line_num() / self.height;
    //     self.get_text_fn(
    //         &p,
    //         line_count,
    //         meta.get_line_num(),
    //         start_page_num,
    //         0,
    //         &mut |x, m1| {
    //             s = x;
    //             m = m1;
    //         },
    //     );
    //     (Some(CacheStr::from_str(s)), m)
    // }

    // pub(crate) fn scroll_next_one_line(
    //     &self,
    //     meta: &EditLineMeta,
    // ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
    //     let (s, l) = self.get_next_line(meta, 1);
    //     if let Some(s) = s {
    //         self.borrow_cache_lines_mut().push(s);
    //         self.borrow_cache_line_meta_mut().push(l);
    //     }
    //     (self.borrow_cache_lines(), self.borrow_cache_line_meta())
    // }

    // pub(crate) fn scroll_pre_one_line(
    //     &self,
    //     meta: &EditLineMeta,
    // ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
    //     let (s, l) = self.get_pre_line(meta, 1);
    //     if let Some(s) = s {
    //         self.borrow_cache_lines_mut().push_front(s);
    //         self.borrow_cache_line_meta_mut().push_front(l);
    //     }
    //     (self.borrow_cache_lines(), self.borrow_cache_line_meta())
    // }

    // pub(crate) fn get_one_page(
    //     &self,
    //     line_num: usize,
    // ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
    //     self.get_line_content(line_num, self.height)
    // }

    // pub(crate) fn get_current_page(&self) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
    //     (self.borrow_cache_lines(), self.borrow_cache_line_meta())
    // }

    // // 从第n行开始获取内容
    // pub(crate) fn get_line_content(
    //     &self,
    //     line_num: usize,
    //     line_count: usize,
    // ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
    //     self.borrow_cache_lines_mut().clear();
    //     self.borrow_cache_line_meta_mut().clear();
    //     self.get_text(line_num, line_count, |txt, meta| {
    //         self.borrow_cache_lines_mut().push(CacheStr::from_str(txt));
    //         self.borrow_cache_line_meta_mut().push(meta);
    //     });
    //     (self.borrow_cache_lines(), self.borrow_cache_line_meta())
    // }

    // pub(crate) fn get_line_content_with_count(
    //     &self,
    //     line_num: usize,
    //     line_count: usize,
    // ) -> (Vec<CacheStr>, Vec<EditLineMeta>) {
    //     let mut lines = Vec::new();
    //     let mut lines_meta = Vec::new();
    //     self.get_text(line_num, line_count, |txt, meta| {
    //         lines.push(CacheStr::from_str(txt));
    //         lines_meta.push(meta);
    //     });
    //     (lines, lines_meta)
    // }

    // fn get_text<'a, F>(&'a self, line_num: usize, line_count: usize, mut f: F)
    // where
    //     F: FnMut(&'a str, EditLineMeta),
    // {
    //     assert!(line_num >= 1);
    //     // 计算页码
    //     let page_num = self.get_page_num(line_num);
    //     // 计算页码
    //     let mut index = (page_num - 1) / PAGE_GROUP;
    //     let page_offset_list = self.borrow_page_offset_list();
    //     let page_offset = if index >= page_offset_list.len() {
    //         index = page_offset_list.len() - 1;
    //         page_offset_list.last().unwrap()
    //     } else {
    //         &page_offset_list[index]
    //     };
    //     let start_page_num = index * PAGE_GROUP;
    //     assert!(line_num >= start_page_num * self.height);
    //     //跳过的行数
    //     let skip_line = line_num;
    //     self.get_text_fn(
    //         &page_offset,
    //         line_count,
    //         start_page_num * self.height,
    //         start_page_num,
    //         skip_line,
    //         &mut f,
    //     );
    // }

    // fn get_text_fn<'a, F>(
    //     &'a self,
    //     page_offset: &PageOffset,
    //     line_count: usize,
    //     start_line_num: usize,
    //     start_page_num: usize,
    //     skip_line: usize,
    //     f: &mut F,
    // ) where
    //     // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
    //     F: FnMut(&'a str, EditLineMeta),
    // {
    //     // if page_offset.line_index >= self.borrow_lines().len() {
    //     //     return;
    //     // }
    //     // let (a, b) = self
    //     //     .borrow_lines_mut()
    //     //     .split_at_mut(page_offset.line_index + 1);
    //     let mut cur_line_count = 0;
    //     let mut line_num = start_line_num;
    //     let mut page_num = start_page_num;

    //     let iter = self
    //         .borrow_lines_mut()
    //         .iter(page_offset.line_index, page_offset.line_start);

    //     for (i, v) in iter.enumerate() {
    //         let line_txt = if i == 0 {
    //             v.text(page_offset.line_start..)
    //         } else {
    //             v.text(..)
    //         };
    //         Self::set_line_txt(
    //             line_txt,
    //             page_offset.line_index + i,
    //             page_offset.line_start,
    //             self.with,
    //             self.height,
    //             self.borrow_page_offset_list_mut(),
    //             &mut line_num,
    //             line_count,
    //             &mut page_num,
    //             &mut cur_line_count,
    //             skip_line,
    //             f,
    //         );
    //         if cur_line_count >= line_count {
    //             return;
    //         }
    //     }

    //     // let line_txt = &a[page_offset.line_index].text()[page_offset.line_start..];
    //     // Self::set_line_txt(
    //     //     line_txt,
    //     //     page_offset.line_index,
    //     //     page_offset.line_start,
    //     //     self.with,
    //     //     self.height,
    //     //     self.borrow_page_offset_list_mut(),
    //     //     &mut line_num,
    //     //     line_count,
    //     //     &mut page_num,
    //     //     &mut cur_line_count,
    //     //     skip_line,
    //     //     f,
    //     // );
    //     // if cur_line_count >= line_count {
    //     //     return;
    //     // }
    //     // // 使用 split_at_mut 获取后续行的可变子切片
    //     // for (i, line) in b.iter_mut().enumerate() {
    //     //     let line_txt = line.text();
    //     //     Self::set_line_txt(
    //     //         line_txt,
    //     //         page_offset.line_index + i + 1,
    //     //         0,
    //     //         self.with,
    //     //         self.height,
    //     //         self.borrow_page_offset_list_mut(),
    //     //         &mut line_num,
    //     //         line_count,
    //     //         &mut page_num,
    //     //         &mut cur_line_count,
    //     //         skip_line,
    //     //         f,
    //     //     );
    //     //     if cur_line_count >= line_count {
    //     //         return;
    //     //     }
    //     // }
    // }

    // fn set_line_txt<'a, F>(
    //     line_txt: &'a str,
    //     line_index: usize,
    //     line_start: usize,
    //     with: usize,
    //     height: usize,
    //     page_offset_list: &mut Vec<PageOffset>,
    //     line_num: &mut usize,
    //     line_count: usize,
    //     page_num: &mut usize,
    //     cur_line_count: &mut usize,
    //     skip_line: usize,
    //     f: &mut F,
    // ) where
    //     // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
    //     F: FnMut(&'a str, EditLineMeta),
    // {
    //     //空行
    //     if line_txt.len() == 0 {
    //         *line_num += 1; //行数加1
    //         if *line_num >= skip_line {
    //             *cur_line_count += 1;
    //             f(
    //                 "",
    //                 EditLineMeta::new(0, 0, *page_num + 1, *line_num, line_index, 0),
    //             );
    //         }
    //         if *line_num % height == 0 {
    //             //到达一页
    //             *page_num += 1; //页数加1
    //             let m = *page_num / PAGE_GROUP;
    //             let n = *page_num % PAGE_GROUP;
    //             if n == 0 && m > page_offset_list.len() - 1 {
    //                 //保存页数的偏移量
    //                 page_offset_list.push(PageOffset {
    //                     line_index,
    //                     line_start: 0,
    //                 });
    //             }
    //         }

    //         if *cur_line_count >= line_count {
    //             return;
    //         }
    //         return;
    //     }

    //     let mut current_width = 0; //
    //     let mut line_offset = 0; //
    //     let mut current_bytes = 0;
    //     let mut char_index = 0;
    //     let mut char_count = 0;

    //     for (i, (byte_index, ch)) in line_txt.char_indices().enumerate() {
    //         let ch_width = ch.width().unwrap_or(0);
    //         //检查是否超过屏幕宽度
    //         if current_width + ch_width > with {
    //             let end = (line_offset + current_bytes).min(line_txt.len());
    //             *line_num += 1; //行数加1
    //             if *line_num >= skip_line {
    //                 *cur_line_count += 1;
    //                 let txt = &line_txt[line_offset..end];
    //                 f(
    //                     txt,
    //                     EditLineMeta::new(
    //                         txt.len(),
    //                         i - char_index,
    //                         *page_num + 1,
    //                         *line_num,
    //                         line_index,
    //                         line_start + line_offset,
    //                     ),
    //                 );
    //             }
    //             if *line_num % height == 0 {
    //                 //到达一页
    //                 *page_num += 1; //页数加1
    //                 let m = *page_num / PAGE_GROUP;
    //                 let n = *page_num % PAGE_GROUP;
    //                 if n == 0 && m > page_offset_list.len() - 1 {
    //                     //保存页数的偏移量
    //                     page_offset_list.push(PageOffset {
    //                         line_index,
    //                         line_start: line_start + byte_index,
    //                     });
    //                 }
    //             }
    //             if *cur_line_count >= line_count {
    //                 return;
    //             }
    //             char_index = i;
    //             line_offset += current_bytes;
    //             current_width = 0;
    //             current_bytes = 0;
    //         }
    //         char_count += 1;
    //         current_width += ch_width;
    //         current_bytes += ch.len_utf8();
    //     }
    //     //当前行没有到达屏幕宽度 但还是一行
    //     if current_bytes > 0 {
    //         *line_num += 1;

    //         if *line_num >= skip_line {
    //             let txt = &line_txt[line_offset..];
    //             *cur_line_count += 1;
    //             f(
    //                 txt,
    //                 EditLineMeta::new(
    //                     txt.len(),
    //                     char_count - char_index,
    //                     *page_num + 1,
    //                     *line_num,
    //                     line_index,
    //                     line_start + line_offset,
    //                 ),
    //             );
    //         }
    //         if *line_num % height == 0 {
    //             *page_num += 1; //页数加1
    //             let m = *page_num / PAGE_GROUP;
    //             let n = *page_num % PAGE_GROUP;
    //             if n == 0 && m > page_offset_list.len() - 1 {
    //                 //保存页数的偏移量
    //                 page_offset_list.push(PageOffset {
    //                     line_index: line_index + 1,
    //                     line_start: 0,
    //                 });
    //             }
    //         }
    //         if *cur_line_count >= line_count {
    //             return;
    //         }
    //     }
    // }
}

pub(crate) struct EditTextBuffer {
    lines: UnsafeCell<Vec<GapBuffer>>, // 每行使用 GapBuffer 存储
    cache_lines: UnsafeCell<RingVec<CacheStr>>, // 缓存行
    cache_line_meta: UnsafeCell<RingVec<EditLineMeta>>, // 缓存行
    page_offset_list: UnsafeCell<Vec<PageOffset>>, // 每页的偏移量
    height: usize,                     //最大行数
    with: usize,                       //最大列数
}

impl EditTextBuffer {}

#[derive(Debug, Clone, Copy)]
struct PageOffset {
    line_index: usize,      //第多少行
    line_offset: usize,     //行在这一行的起始位置
    line_file_start: usize, //这一行在整个文件的起始位置
}

// impl EditTextBuffer {
//     pub(crate) fn from_file_path<P: AsRef<Path>>(
//         filename: P,
//         height: usize,
//         with: usize,
//     ) -> ChapResult<EditTextBuffer> {
//         let lines = read_lines(filename)?;
//         let mut gap_buffers: Vec<GapBuffer> = Vec::new();
//         for line in lines {
//             if let Ok(content) = line {
//                 let mut gap_buffer = GapBuffer::new(content.len() + 5);
//                 gap_buffer.insert(0, &content);
//                 gap_buffers.push(gap_buffer);
//             }
//         }

//         Ok(EditTextBuffer {
//             lines: UnsafeCell::new(gap_buffers),
//             cache_lines: UnsafeCell::new(RingVec::new(height)),
//             cache_line_meta: UnsafeCell::new(RingVec::new(height)),
//             page_offset_list: UnsafeCell::new(vec![PageOffset {
//                 line_index: 0,
//                 line_offset: 0,
//                 line_start: 0,
//             }]),
//             height: height,
//             with: with,
//         })
//     }

//     fn borrow_lines(&self) -> &Vec<GapBuffer> {
//         unsafe { &*self.lines.get() }
//     }

//     fn borrow_lines_mut(&self) -> &mut Vec<GapBuffer> {
//         unsafe { &mut *self.lines.get() }
//     }

//     fn borrow_page_offset_list(&self) -> &Vec<PageOffset> {
//         unsafe { &*self.page_offset_list.get() }
//     }

//     fn borrow_page_offset_list_mut(&self) -> &mut Vec<PageOffset> {
//         unsafe { &mut *self.page_offset_list.get() }
//     }

//     fn borrow_cache_lines(&self) -> &RingVec<CacheStr> {
//         unsafe { &*self.cache_lines.get() }
//     }

//     fn borrow_cache_lines_mut(&self) -> &mut RingVec<CacheStr> {
//         unsafe { &mut *self.cache_lines.get() }
//     }

//     fn borrow_cache_line_meta(&self) -> &RingVec<EditLineMeta> {
//         unsafe { &*self.cache_line_meta.get() }
//     }

//     fn borrow_cache_line_meta_mut(&self) -> &mut RingVec<EditLineMeta> {
//         unsafe { &mut *self.cache_line_meta.get() }
//     }

//     pub(crate) fn get_text_len(&self, index: usize) -> usize {
//         if index >= self.borrow_lines().len() {
//             return 0;
//         }
//         self.borrow_lines()[index].text_len()
//     }

//     // 计算页码，等同于向上取整
//     fn get_page_num(&self, num: usize) -> usize {
//         (num + self.height - 1) / self.height
//     }

//     // 计算行数，等同于向上取整
//     fn calculate_lines(text_len: usize, with: usize) -> usize {
//         if text_len == 0 {
//             return 1;
//         }
//         (text_len as f64 / with as f64).ceil() as usize
//     }

//     // 计算光标所在行和列
//     pub(crate) fn calculate_x_y(&self, cursor_y: usize, cursor_x: usize) -> (usize, usize) {
//         let mut line_count = 0;
//         let mut line_index = 0;
//         let mut shirt = 0;
//         // 计算光标所在行
//         let y = cursor_y + 1;
//         for (i, b) in self.borrow_lines().iter().enumerate() {
//             let cur_line_count = Self::calculate_lines(b.text_len(), self.with);
//             line_count += cur_line_count;
//             line_index = i;
//             if y <= line_count {
//                 shirt = cur_line_count - (line_count - y) - 1;
//                 break;
//             }
//         }
//         let line_offset = shirt * self.with + cursor_x;
//         (line_index, line_offset)
//     }

//     pub(crate) fn save<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()> {
//         let backup_name = Self::get_backup_name(&filepath)?;
//         self.make_backup(&backup_name)?;
//         Self::rename_backup(&filepath, &backup_name)?;
//         Ok(())
//     }

//     fn get_backup_name<P: AsRef<Path>>(filepath: P) -> ChapResult<PathBuf> {
//         let mut path = filepath.as_ref().to_path_buf();
//         if let Some(file_name) = path.file_name() {
//             path.set_file_name(format!(".{}.{}", file_name.to_string_lossy(), "chap"));
//         }
//         Ok(path)
//     }

//     fn make_backup<P: AsRef<Path>>(&mut self, backup_name: P) -> ChapResult<()> {
//         // 备份文件
//         let file = std::fs::File::create(backup_name).unwrap();
//         let mut w = std::io::BufWriter::new(&file);
//         for line in self.borrow_lines_mut().iter_mut() {
//             let txt = line.text(..);
//             w.write(txt.as_bytes()).unwrap();
//             w.write(b"\n").unwrap();
//         }
//         w.flush()?;
//         Ok(())
//     }

//     fn rename_backup<P1: AsRef<Path>, P2: AsRef<Path>>(
//         filepath: P1,
//         backup_name: P2,
//     ) -> ChapResult<()> {
//         fs::rename(backup_name, filepath)?;
//         Ok(())
//     }

//     //通过 line_index 获取页数
//     // fn get_page_num_from_line_index(&self, line_index: usize) -> usize {
//     //     for p in self.borrow_page_offset_list().iter() {
//     //         if line_index < p.line_index {
//     //             return p.line_index;
//     //         }
//     //     }
//     // }

//     // 插入字符
//     // 计算光标所在行
//     // 计算光标所在列
//     pub(crate) fn insert(
//         &self,
//         cursor_y: usize,
//         cursor_x: usize,
//         line_meta: &EditLineMeta,
//         c: char,
//     ) {
//         let (line_index, line_offset) = (
//             line_meta.get_line_index(),
//             line_meta.get_line_offset() + cursor_x,
//         );

//         let mut buf = [0u8; 4]; // 一个 char 最多需要 4 个字节存储 UTF-8 编码
//         let s: &str = c.encode_utf8(&mut buf);
//         let line = &mut self.borrow_lines_mut()[line_index];
//         //如果line_offset大于文本长度 要填充空格
//         if line_offset > line.text_len() {
//             let gap_len = line_offset - line.text_len();
//             line.insert(line.text_len(), " ".repeat(gap_len).as_str());
//         }
//         line.insert(line_offset, s);
//         //切断page_offset_list 索引
//         let page_offset_list = self.borrow_page_offset_list_mut();
//         unsafe { page_offset_list.set_len(line_meta.get_page_num()) };
//         self.borrow_cache_lines_mut().clear();
//         self.borrow_cache_line_meta_mut().clear();
//     }

//     // 插入换行
//     pub(crate) fn insert_newline(
//         &self,
//         cursor_y: usize,
//         cursor_x: usize,
//         line_meta: &EditLineMeta,
//     ) {
//         // let (line_index, line_offset) = self.calculate_x_y(cursor_y, cursor_x);
//         let (line_index, line_offset) = (
//             line_meta.get_line_index(),
//             line_meta.get_line_offset() + cursor_x,
//         );
//         //    println!("line_index: {}, line_offset: {}", line_index, line_offset);
//         let line_txt = self.borrow_lines_mut()[line_index].text(..);
//         let line_len = line_txt.len();
//         {
//             if line_offset > line_len {
//                 // 如果光标不在行尾，插入新行
//                 let new_gap_buffer = GapBuffer::new(10);
//                 self.borrow_lines_mut()
//                     .insert(line_index + 1, new_gap_buffer);
//             } else {
//                 let b = &line_txt[line_offset..];
//                 let mut new_gap_buffer = GapBuffer::new(b.len() + 5);
//                 new_gap_buffer.insert(0, b);
//                 self.borrow_lines_mut()
//                     .insert(line_index + 1, new_gap_buffer);
//             }
//         }

//         {
//             if line_len > line_offset {
//                 let delete_len = line_len - line_offset;
//                 // 删除当前行的剩余部分
//                 self.borrow_lines_mut()[line_index].delete(line_len, delete_len);
//             }
//         }
//         let page_offset_list = self.borrow_page_offset_list_mut();
//         unsafe { page_offset_list.set_len(line_meta.get_page_num()) };
//         self.borrow_cache_lines_mut().clear();
//         self.borrow_cache_line_meta_mut().clear();
//     }

//     // 删除光标前一个字符
//     pub(crate) fn backspace(&self, cursor_y: usize, cursor_x: usize, line_meta: &EditLineMeta) {
//         // let (line_index, line_offset) = self.calculate_x_y(cursor_y, cursor_x);
//         let (line_index, line_offset) = (
//             line_meta.get_line_index(),
//             line_meta.get_line_offset() + cursor_x,
//         );
//         if self.borrow_lines_mut()[line_index].text_len() == 0 && line_offset == 0 {
//             //删除一行
//             self.borrow_lines_mut().remove(line_index);
//             return;
//         }
//         //表示当前行和前一行合并
//         if line_offset == 0 {
//             if line_index == 0 {
//                 return;
//             }
//             //用.split_at_mut(position)修改代码
//             let (pre_lines, cur_lines) = self.borrow_lines_mut().split_at_mut(line_index);
//             let pre_line = &mut pre_lines[line_index - 1];
//             if pre_line.text_len() == 0 {
//                 self.borrow_lines_mut().remove(line_index - 1);
//                 return;
//             } else {
//                 let cur_line = &mut cur_lines[0];
//                 let cur_line_txt = cur_line.text(..);
//                 pre_line.insert(pre_line.text_len(), cur_line_txt);
//                 self.borrow_lines_mut().remove(line_index);
//             }
//             return;
//         }
//         self.borrow_lines_mut()[line_index].backspace(line_offset);
//         let page_offset_list = self.borrow_page_offset_list_mut();
//         unsafe { page_offset_list.set_len(line_meta.get_page_num()) };
//         self.borrow_cache_lines_mut().clear();
//         self.borrow_cache_line_meta_mut().clear();
//     }

//     //从当前行开始获取后面n行
//     pub(crate) fn get_pre_line<'a>(
//         &'a self,
//         meta: &EditLineMeta,
//         line_count: usize,
//     ) -> (Option<CacheStr>, EditLineMeta) {
//         if meta.get_line_num() == 1 {
//             return (None, EditLineMeta::default());
//         }
//         let mut s = "";
//         let mut m = EditLineMeta::default();
//         self.get_text(meta.get_line_num() - line_count, line_count, |txt, meta| {
//             s = txt;
//             m = meta;
//         });
//         (Some(CacheStr::from_str(s)), m)
//     }

//     //从当前行开始获取后面n行
//     pub(crate) fn get_next_line<'a>(
//         &'a self,
//         meta: &EditLineMeta,
//         line_count: usize,
//     ) -> (Option<CacheStr>, EditLineMeta) {
//         let mut line_index = meta.get_line_index();
//         let mut line_end = meta.get_line_end();
//         let mut line_start = meta.get_line_start();

//         if line_index == self.borrow_lines().len() - 1 && line_end == self.get_text_len(line_index)
//         {
//             return (None, EditLineMeta::default());
//         }

//         let line = &self.borrow_lines()[line_index];
//         //如果当前行的长度大于光标位置，表示光标在当前行
//         if line_end == line.text_len() {
//             line_start = line_end;
//             line_end = 0;
//             line_index += 1;

//         }

//         let p = PageOffset {
//             line_index: line_index,
//             line_offset: line_end,
//             line_start:line_start,
//         };
//         let mut s = "";
//         let mut m = EditLineMeta::default();
//         let start_page_num = meta.get_line_num() / self.height;
//         self.get_text_fn(
//             &p,
//             line_count,
//             meta.get_line_num(),
//             start_page_num,
//             0,
//             &mut |x, m1| {
//                 s = x;
//                 m = m1;
//             },
//         );
//         (Some(CacheStr::from_str(s)), m)
//     }

//     pub(crate) fn scroll_next_one_line(
//         &self,
//         meta: &EditLineMeta,
//     ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
//         let (s, l) = self.get_next_line(meta, 1);
//         if let Some(s) = s {
//             self.borrow_cache_lines_mut().push(s);
//             self.borrow_cache_line_meta_mut().push(l);
//         }
//         (self.borrow_cache_lines(), self.borrow_cache_line_meta())
//     }

//     pub(crate) fn scroll_pre_one_line(
//         &self,
//         meta: &EditLineMeta,
//     ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
//         let (s, l) = self.get_pre_line(meta, 1);
//         if let Some(s) = s {
//             self.borrow_cache_lines_mut().push_front(s);
//             self.borrow_cache_line_meta_mut().push_front(l);
//         }
//         (self.borrow_cache_lines(), self.borrow_cache_line_meta())
//     }

//     pub(crate) fn get_one_page(
//         &self,
//         line_num: usize,
//     ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
//         self.get_line_content(line_num, self.height)
//     }

//     pub(crate) fn get_current_page(&self) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
//         (self.borrow_cache_lines(), self.borrow_cache_line_meta())
//     }

//     // 从第n行开始获取内容
//     pub(crate) fn get_line_content(
//         &self,
//         line_num: usize,
//         line_count: usize,
//     ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
//         self.borrow_cache_lines_mut().clear();
//         self.borrow_cache_line_meta_mut().clear();
//         self.get_text(line_num, line_count, |txt, meta| {
//             self.borrow_cache_lines_mut().push(CacheStr::from_str(txt));
//             self.borrow_cache_line_meta_mut().push(meta);
//         });
//         (self.borrow_cache_lines(), self.borrow_cache_line_meta())
//     }

//     pub(crate) fn get_line_content_with_count(
//         &self,
//         line_num: usize,
//         line_count: usize,
//     ) -> (Vec<CacheStr>, Vec<EditLineMeta>) {
//         let mut lines = Vec::new();
//         let mut lines_meta = Vec::new();
//         self.get_text(line_num, line_count, |txt, meta| {
//             lines.push(CacheStr::from_str(txt));
//             lines_meta.push(meta);
//         });
//         (lines, lines_meta)
//     }

//     fn get_text<'a, F>(&'a self, line_num: usize, line_count: usize, mut f: F)
//     where
//         F: FnMut(&'a str, EditLineMeta),
//     {
//         assert!(line_num >= 1);
//         // 计算页码
//         let page_num = self.get_page_num(line_num);
//         // 计算页码
//         let mut index = (page_num - 1) / PAGE_GROUP;
//         let page_offset_list = self.borrow_page_offset_list();
//         let page_offset = if index >= page_offset_list.len() {
//             index = page_offset_list.len() - 1;
//             page_offset_list.last().unwrap()
//         } else {
//             &page_offset_list[index]
//         };
//         let start_page_num = index * PAGE_GROUP;
//         assert!(line_num >= start_page_num * self.height);
//         //跳过的行数
//         let skip_line = line_num;
//         self.get_text_fn(
//             &page_offset,
//             line_count,
//             start_page_num * self.height,
//             start_page_num,
//             skip_line,
//             &mut f,
//         );
//     }

//     fn get_text_fn<'a, F>(
//         &'a self,
//         page_offset: &PageOffset,
//         line_count: usize,
//         start_line_num: usize,
//         start_page_num: usize,
//         skip_line: usize,
//         f: &mut F,
//     ) where
//         // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
//         F: FnMut(&'a str, EditLineMeta),
//     {
//         if page_offset.line_index >= self.borrow_lines().len() {
//             return;
//         }
//         let (a, b) = self
//             .borrow_lines_mut()
//             .split_at_mut(page_offset.line_index + 1);
//         let mut cur_line_count = 0;
//         let mut line_num = start_line_num;
//         let mut page_num = start_page_num;

//         let line_txt = &a[page_offset.line_index].text(page_offset.line_offset..);
//         Self::set_line_txt(
//             line_txt,
//             page_offset.line_index,
//             page_offset.line_offset,
//             self.with,
//             self.height,
//             self.borrow_page_offset_list_mut(),
//             &mut line_num,
//             line_count,
//             &mut page_num,
//             &mut cur_line_count,
//             skip_line,
//             f,
//         );
//         if cur_line_count >= line_count {
//             return;
//         }
//         // 使用 split_at_mut 获取后续行的可变子切片
//         for (i, line) in b.iter_mut().enumerate() {
//             let line_txt = line.text(..);
//             Self::set_line_txt(
//                 line_txt,
//                 page_offset.line_index + i + 1,
//                 0,
//                 self.with,
//                 self.height,
//                 self.borrow_page_offset_list_mut(),
//                 &mut line_num,
//                 line_count,
//                 &mut page_num,
//                 &mut cur_line_count,
//                 skip_line,
//                 f,
//             );
//             if cur_line_count >= line_count {
//                 return;
//             }
//         }
//     }

//     fn set_line_txt<'a, F>(
//         line_txt: &'a str,
//         line_index: usize,
//         line_start: usize,
//         with: usize,
//         height: usize,
//         page_offset_list: &mut Vec<PageOffset>,
//         line_num: &mut usize,
//         line_count: usize,
//         page_num: &mut usize,
//         cur_line_count: &mut usize,
//         skip_line: usize,
//         f: &mut F,
//     ) where
//         // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
//         F: FnMut(&'a str, EditLineMeta),
//     {
//         //空行
//         if line_txt.len() == 0 {
//             *line_num += 1; //行数加1
//             if *line_num >= skip_line {
//                 *cur_line_count += 1;
//                 f(
//                     "",
//                     EditLineMeta::new(0, 0, *page_num + 1, *line_num, line_index, 0, 0, 0),
//                 );
//             }
//             if *line_num % height == 0 {
//                 //到达一页
//                 *page_num += 1; //页数加1
//                 let m = *page_num / PAGE_GROUP;
//                 let n = *page_num % PAGE_GROUP;
//                 if n == 0 && m > page_offset_list.len() - 1 {
//                     //保存页数的偏移量
//                     page_offset_list.push(PageOffset {
//                         line_index,
//                         line_offset: 0,
//                         line_start: 0,
//                     });
//                 }
//             }

//             if *cur_line_count >= line_count {
//                 return;
//             }
//             return;
//         }

//         let mut current_width = 0; //
//         let mut line_offset = 0; //
//         let mut current_bytes = 0;
//         let mut char_index = 0;
//         let mut char_count = 0;

//         for (i, (byte_index, ch)) in line_txt.char_indices().enumerate() {
//             let ch_width = ch.width().unwrap_or(0);
//             //检查是否超过屏幕宽度
//             if current_width + ch_width > with {
//                 let end = (line_offset + current_bytes).min(line_txt.len());
//                 *line_num += 1; //行数加1
//                 if *line_num >= skip_line {
//                     *cur_line_count += 1;
//                     let txt = &line_txt[line_offset..end];
//                     f(
//                         txt,
//                         EditLineMeta::new(
//                             txt.len(),
//                             i - char_index,
//                             *page_num + 1,
//                             *line_num,
//                             line_index,
//                             line_start + line_offset,
//                             0,
//                             0,
//                         ),
//                     );
//                 }
//                 if *line_num % height == 0 {
//                     //到达一页
//                     *page_num += 1; //页数加1
//                     let m = *page_num / PAGE_GROUP;
//                     let n = *page_num % PAGE_GROUP;
//                     if n == 0 && m > page_offset_list.len() - 1 {
//                         //保存页数的偏移量
//                         page_offset_list.push(PageOffset {
//                             line_index,
//                             line_offset: line_start + byte_index,
//                         });
//                     }
//                 }
//                 if *cur_line_count >= line_count {
//                     return;
//                 }
//                 char_index = i;
//                 line_offset += current_bytes;
//                 current_width = 0;
//                 current_bytes = 0;
//             }
//             char_count += 1;
//             current_width += ch_width;
//             current_bytes += ch.len_utf8();
//         }
//         //当前行没有到达屏幕宽度 但还是一行
//         if current_bytes > 0 {
//             *line_num += 1;

//             if *line_num >= skip_line {
//                 let txt = &line_txt[line_offset..];
//                 *cur_line_count += 1;
//                 f(
//                     txt,
//                     EditLineMeta::new(
//                         txt.len(),
//                         char_count - char_index,
//                         *page_num + 1,
//                         *line_num,
//                         line_index,
//                         line_start + line_offset,
//                         0,
//                         0,
//                     ),
//                 );
//             }
//             if *line_num % height == 0 {
//                 *page_num += 1; //页数加1
//                 let m = *page_num / PAGE_GROUP;
//                 let n = *page_num % PAGE_GROUP;
//                 if n == 0 && m > page_offset_list.len() - 1 {
//                     //保存页数的偏移量
//                     page_offset_list.push(PageOffset {
//                         line_index: line_index + 1,
//                         line_offset: 0,
//                     });
//                 }
//             }
//             if *cur_line_count >= line_count {
//                 return;
//             }
//         }
//     }
// }

#[cfg(test)]
mod tests {

    use super::*;
    use std::fs::File;

    #[test]
    fn text_hex() {
        let mut hex = HexText::from_file_path("/root/aa.txt").unwrap();
        println!("hex len: {:?}", hex.buffer.get_buffer());
        let iter = hex.iter(0, 0, 0);
        for i in iter {
            println!("i: {:?}", i.line);
        }
    }

    #[test]
    fn test_print() {
        let file = File::open("/root/aa.txt").unwrap();
        let mmap = unsafe { Mmap::map(&file).unwrap() };
        println!(" mmap len: {:?}", mmap.len());
        let mmap_text = MmapText::new(mmap);

        let text = TextWarp::new(mmap_text, 2, 5, TextType::Char);
        let (s, c) = text.get_one_page(1);
        for (i, l) in s.iter().enumerate() {
            println!("l: {:?},{:?}", l.as_str(), c.get(i));
        }

        for p in text.borrow_page_offset_list().iter() {
            println!("p:{:?}", p)
        }

        let (s, c) = text.get_next_line(c.last().unwrap(), 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);

        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        // for p in text.borrow_page_offset_list().iter() {
        //     println!("p:{:?}", p)
        // }

        // let (s, c) = text.get_one_page(3);
        // for (i, l) in s.iter().enumerate() {
        //     println!("l: {:?},{:?}", l.as_str(), c.get(i));
        // }
        // for p in text.borrow_page_offset_list().iter() {
        //     println!("p:{:?}", p)
        // }

        // let (s, c) = text.get_one_page(4);
        // for (i, l) in s.iter().enumerate() {
        //     println!("l: {:?},{:?}", l.as_str(), c.get(i));
        // }
        // for p in text.borrow_page_offset_list().iter() {
        //     println!("p:{:?}", p)
        // }
    }

    // #[test]
    // fn test_print() {
    //     let mut b = EditTextBuffer::from_file_path("/root/aa.txt", 2, 5).unwrap();

    //     let c = {
    //         let (s, c) = b.get_one_page(1);
    //         for (i, l) in s.iter().enumerate() {
    //             println!("l: {:?},{:?}", l.as_str(), c.get(i));
    //         }
    //         let (s, c) = b.get_one_page(3);
    //         for (i, l) in s.iter().enumerate() {
    //             println!("l: {:?},{:?}", l.as_str(), c.get(i));
    //         }
    //         let (s, c) = b.get_one_page(5);
    //         for (i, l) in s.iter().enumerate() {
    //             println!("l: {:?},{:?}", l.as_str(), c.get(i));
    //         }
    //         let (s, c) = b.get_one_page(7);
    //         for (i, l) in s.iter().enumerate() {
    //             println!("l: {:?},{:?}", l.as_str(), c.get(i));
    //         }

    //         let (s, c) = b.get_one_page(1);
    //         for (i, l) in s.iter().enumerate() {
    //             println!("l: {:?},{:?}", l.as_str(), c.get(i));
    //         }

    //         for p in b.borrow_page_offset_list().iter() {
    //             println!("p:{:?}", p)
    //         }
    //         c
    //     };
    //     let y = 0;
    //     let x = 5;
    //     b.insert(y, x, c.get(y).unwrap(), 'a');
    //     for p in b.borrow_page_offset_list().iter() {
    //         println!("p1:{:?}", p)
    //     }
    //     // let (s, m) = b.get_next_line(&c.last().unwrap(), 1);
    //     // println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     // let (s, m) = b.get_next_line(&m, 1);
    //     // println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     // let (s, m) = b.get_next_line(&m, 1);
    //     // println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     // let (s, m) = b.get_next_line(&m, 1);
    //     // println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     // let (s, m) = b.get_next_line(&m, 1);
    //     // println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     // let (s, m) = b.get_next_line(&m, 1);
    //     // println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     // let (s, m) = b.get_next_line(&m, 1);
    //     // println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     // let (s, m) = b.get_next_line(&m, 1);
    //     // println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     // let (s, m) = b.get_next_line(&m, 1);
    //     // println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     // let (s, m) = b.get_next_line(&m, 1);
    //     // println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     // let (s, m) = b.get_next_line(&m, 1);
    //     // println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     // let (s, m) = b.get_next_line(&m, 1);
    //     // println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     // let (s, m) = b.get_next_line(&m, 1);
    //     // println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     // let (s, m) = b.get_next_line(&m, 1);
    //     // println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     // let (s, m) = b.get_next_line(&m, 1);
    //     // println!("{:?},{:?}", s.unwrap().as_str(), m);
    // }

    // #[test]
    // fn test_print2() {
    //     let mut b =
    //         EditTextBuffer::from_file_path("/opt/rsproject/chappie/src/chap.rs", 10, 90).unwrap();

    //     let (s, c) = b.get_line_content_with_count(1, 100);
    //     for (i, l) in s.iter().enumerate() {
    //         println!("ll: {:?},{:?}", l.as_str(), c.get(i));
    //     }

    //     // let c = {
    //     //     let (s, c) = b.get_one_page(1);
    //     //     for (i, l) in s.iter().enumerate() {
    //     //         println!("l: {:?},{:?}", l.as_str(), c.get(i));
    //     //     }

    //     //     // for p in b.page_offset_list.iter() {
    //     //     //     println!("p:{:?}", p)
    //     //     // }
    //     //     c
    //     // };

    //     // b.scroll_next_one_line(c.last().unwrap());

    //     // let (s, c) = b.get_current_page();
    //     // for (i, l) in s.iter().enumerate() {
    //     //     println!("n: {:?},{:?}", l.as_str(), c.get(i));
    //     // }
    //     // println!("=====================================");

    //     // b.scroll_next_one_line(c.last().unwrap());

    //     // let (s, c) = b.get_current_page();
    //     // for (i, l) in s.iter().enumerate() {
    //     //     println!("n: {:?},{:?}", l.as_str(), c.get(i));
    //     // }
    //     // println!("=====================================");
    //     // b.scroll_next_one_line(c.last().unwrap());

    //     // let (s, c) = b.get_current_page();
    //     // for (i, l) in s.iter().enumerate() {
    //     //     println!("n: {:?},{:?}", l.as_str(), c.get(i));
    //     // }
    //     // println!("=====================================");
    //     // b.scroll_pre_one_line(c.get(0).unwrap());

    //     // let (s, c) = b.get_current_page();
    //     // for (i, l) in s.iter().enumerate() {
    //     //     println!("p: {:?},{:?}", l.as_str(), c.get(i));
    //     // }
    //     // println!("=====================================");
    //     // b.scroll_pre_one_line(c.get(0).unwrap());

    //     // let (s, c) = b.get_current_page();
    //     // for (i, l) in s.iter().enumerate() {
    //     //     println!("p: {:?},{:?}", l.as_str(), c.get(i));
    //     // }
    //     // println!("=====================================");
    //     // b.scroll_pre_one_line(c.get(0).unwrap());

    //     // let (s, c) = b.get_current_page();
    //     // for (i, l) in s.iter().enumerate() {
    //     //     println!("p: {:?},{:?}", l.as_str(), c.get(i));
    //     // }
    //     // println!("=====================================");
    // }

    // #[test]
    // fn test_insert() {
    //     let mut b = EditTextBuffer::from_file_path("/root/aa.txt", 2, 5).unwrap();

    //     {
    //         let (s, c) = b.get_line_content_with_count(1, 10);
    //         for l in s.iter() {
    //             println!("l: {:?}", l.as_str());
    //         }

    //         let y = 0;
    //         let x = 5;
    //         b.insert(y, x, c.get(y).unwrap(), 'b');
    //     }

    //     // b.insert(y, x + 1, 'b');
    //     // b.insert(y, x + 1 + 1, 'c');
    //     // b.insert(y, x + 1 + 1 + 1, 'd');
    //     // b.insert(y, x + 1 + 1 + 1 + 1, 'e');
    //     {
    //         let (s, c) = b.get_line_content_with_count(1, 10);
    //         for l in s.iter() {
    //             println!("l1: {:?}", l.as_str());
    //         }
    //     }
    // }

    // #[test]
    // fn test_insert_newline() {
    //     let mut b = EditTextBuffer::from_file_path("/root/aa.txt", 2, 5).unwrap();

    //     {
    //         let (s, c) = b.get_line_content(1, 10);
    //         for l in s.iter() {
    //             println!("l: {:?}", l.as_str());
    //         }
    //         let cursor_y = 1;
    //         let cursor_x = 4;
    //         b.insert_newline(cursor_y, cursor_x, c.get(cursor_y).unwrap());
    //         {
    //             let (s, c) = b.get_line_content(1, 10);
    //             for l in s.iter() {
    //                 println!("l1: {:?}", l.as_str());
    //             }
    //         }
    //         {
    //             let (s, c) = b.get_line_content(1, 10);
    //             for l in s.iter() {
    //                 println!("l1: {:?}", l.as_str());
    //             }
    //         }
    //     }
    // }

    // #[test]
    // fn test_backspace() {
    //     let mut b = EditTextBuffer::from_file_path("/root/aa.txt", 10, 90).unwrap();

    //     {
    //         let (s, c) = b.get_line_content(1, 10);
    //         for l in s.iter() {
    //             println!("l: {:?}", l.as_str());
    //         }
    //         let cursor_y = 6;
    //         let cursor_x = 0;
    //         b.backspace(cursor_y, cursor_x, c.get(cursor_y).unwrap());
    //         {
    //             let (s, c) = b.get_line_content(1, 10);
    //             for l in s.iter() {
    //                 println!("l1: {:?}", l.as_str());
    //             }
    //         }
    //         {
    //             let (s, c) = b.get_line_content(1, 10);
    //             for l in s.iter() {
    //                 println!("l1: {:?}", l.as_str());
    //             }
    //         }
    //     }
    // }

    // #[test]
    // fn test_calculate_lines() {
    //     let txt = "12345678910";
    //     let line_count = EditTextBuffer::calculate_lines(txt.len(), 5);
    //     println!("line_count: {}", line_count);
    // }

    // #[test]
    // fn test_get_backup_name() {
    //     let filepath = "/root/aa/12345678910";
    //     let name = EditTextBuffer::get_backup_name(filepath).unwrap();
    //     println!("name: {:?}", name);
    // }

    #[test]
    fn test_ringcache() {
        let mut ring_cache = RingVec::<usize>::new(10);
        for i in 0..11 {
            ring_cache.push(i);
        }

        for i in ring_cache.iter() {
            println!("i: {:?}", i);
        }

        ring_cache.push_front(0);

        for i in ring_cache.iter() {
            println!("i1: {:?}", i);
        }

        ring_cache.push_front(11);
        ring_cache.push_front(12);
        ring_cache.push_front(13);
        ring_cache.push_front(14);
        ring_cache.push_front(15);
        ring_cache.push_front(16);
        ring_cache.push_front(17);
        ring_cache.push_front(18);
        ring_cache.push_front(19);

        for i in ring_cache.iter() {
            println!("i2: {:?}", i);
        }

        println!("{:?}", ring_cache.get(0));
        println!("{:?}", ring_cache.get(1));
        println!("{:?}", ring_cache.get(2));
        println!("{:?}", ring_cache.get(3));
        println!("{:?}", ring_cache.get(4));
        println!("{:?}", ring_cache.get(5));
        println!("{:?}", ring_cache.get(6));
        println!("{:?}", ring_cache.get(7));
        println!("{:?}", ring_cache.get(8));
        println!("{:?}", ring_cache.get(9));
        println!("{:?}", ring_cache.get(10));
        println!("{:?}", ring_cache.get(11));

        // println!("{:?}", ring_cache.last());
    }

    // #[test]
    // fn test_print3() {
    //     let mut b = EditTextBuffer::from_file_path("/root/aa.txt", 2, 5).unwrap();

    //     let c = {
    //         let (s, c) = b.get_line_content_with_count(1, 11);
    //         for (i, l) in s.iter().enumerate() {
    //             println!("l: {:?}{:?}", l.as_str(), c.get(i).unwrap());
    //         }

    //         for p in b.borrow_page_offset_list().iter() {
    //             println!("p:{:?}", p)
    //         }
    //         c
    //     };
    //     let (s, m) = b.get_pre_line(&c.last().unwrap(), 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);

    //     // for p in b.borrow_page_offset_list().iter() {
    //     //     println!("p:{:?}", p)
    //     // }
    // }

    // #[test]
    // fn test_print4() {
    //     let mut b = EditTextBuffer::from_file_path("/root/aa.txt", 2, 5).unwrap();

    //     let c = {
    //         let (s, c) = b.get_line_content_with_count(1, 11);
    //         for (i, l) in s.iter().enumerate() {
    //             println!("l0: {:?}{:?}", l.as_str(), c.get(i).unwrap());
    //         }

    //         let (s, c) = b.get_line_content_with_count(4, 11);
    //         for (i, l) in s.iter().enumerate() {
    //             println!("l1: {:?}{:?}", l.as_str(), c.get(i).unwrap());
    //         }

    //         for p in b.borrow_page_offset_list().iter() {
    //             println!("p:{:?}", p)
    //         }
    //         c
    //     };
    // }

    #[test]
    fn test_mmap() {
        let path = "/root/aa.txt";
        let file = std::fs::File::open(path).unwrap();
        let mmap = unsafe { Mmap::map(&file).unwrap() };
        //let
    }
}
