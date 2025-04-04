use crate::util::read_lines;
use crate::{error::ChapResult, gap_buffer::GapBuffer};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use unicode_width::UnicodeWidthChar;
const PAGE_GROUP: usize = 1;
use crate::text::LineMeta;

#[derive(Debug, Default)]
pub(crate) struct EditLineMeta {
    //  txt: &'a str,
    txt_len: usize,
    page_num: usize,
    line_num: usize,
    line_index: usize,
    line_offset: usize,
}

impl EditLineMeta {
    pub(crate) fn new(
        txt_len: usize,
        page_num: usize,
        line_num: usize,
        line_index: usize,
        line_offset: usize,
    ) -> EditLineMeta {
        EditLineMeta {
            txt_len,
            page_num,
            line_num,
            line_index,
            line_offset,
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
}

struct CacheStr {
    data: NonNull<str>,
    len: usize,
}

impl CacheStr {
    fn from_str(s: &str) -> Self {
        let ptr = s as *const str as *mut str; // 获取 &str 的指针
        let len = s.len(); // 获取 &str 的长度
        let non_null_ptr = unsafe { NonNull::new_unchecked(ptr) }; // 创建 NonNull<str>
        CacheStr {
            data: non_null_ptr,
            len,
        }
    }

    // 从 CacheStr 获取 &str
    fn as_str(&self) -> &str {
        unsafe { self.data.as_ref() }
    }
}

pub(crate) struct EditTextBuffer {
    lines: Vec<GapBuffer>,      // 每行使用 GapBuffer 存储
    cache_lines: Vec<CacheStr>, // 缓存行
    cursor_x: usize,
    cursor_y: usize, // 光标位置，cursorY 表示行号，cursorX 表示行内位置
    page_offset_list: Vec<PageOffset>, // 每页的偏移量
    height: usize,   //最大行数
    with: usize,     //最大列数
}

#[derive(Debug, Clone, Copy)]
struct PageOffset {
    line_index: usize,
    line_start: usize,
}

impl EditTextBuffer {
    pub(crate) fn from_file_path<P: AsRef<Path>>(
        filename: P,
        height: usize,
        with: usize,
    ) -> ChapResult<EditTextBuffer> {
        let lines = read_lines(filename)?;
        let mut gap_buffers: Vec<GapBuffer> = Vec::new();
        for line in lines {
            if let Ok(content) = line {
                let mut gap_buffer = GapBuffer::new(content.len() + 5);
                gap_buffer.insert(0, &content);
                gap_buffers.push(gap_buffer);
            }
        }

        Ok(EditTextBuffer {
            lines: gap_buffers,
            cache_lines: Vec::new(),
            cursor_x: 0,
            cursor_y: 0,
            page_offset_list: vec![PageOffset {
                line_index: 0,
                line_start: 0,
            }],
            height: height,
            with: with,
        })
    }

    pub(crate) fn get_text_len(&self, index: usize) -> usize {
        if index >= self.lines.len() {
            return 0;
        }
        self.lines[index].text_len()
    }

    // 计算页码，等同于向上取整
    fn get_page_num(&mut self, num: usize) -> usize {
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
    pub(crate) fn calculate_x_y(&self, cursor_y: usize, cursor_x: usize) -> (usize, usize) {
        let mut line_count = 0;
        let mut line_index = 0;
        let mut shirt = 0;
        // 计算光标所在行
        let y = cursor_y + 1;
        for (i, b) in self.lines.iter().enumerate() {
            let cur_line_count = Self::calculate_lines(b.text_len(), self.with);
            line_count += cur_line_count;
            line_index = i;
            if y <= line_count {
                shirt = cur_line_count - (line_count - y) - 1;
                break;
            }
        }
        let line_offset = shirt * self.with + cursor_x;
        (line_index, line_offset)
    }

    pub(crate) fn save<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()> {
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
        for line in &mut self.lines {
            let txt = line.text();
            w.write(txt.as_bytes()).unwrap();
            w.write(b"\n").unwrap();
        }
        w.flush()?;
        Ok(())
    }

    fn rename_backup<P1: AsRef<Path>, P2: AsRef<Path>>(
        filepath: P1,
        backup_name: P2,
    ) -> ChapResult<()> {
        fs::rename(backup_name, filepath)?;
        Ok(())
    }

    // 插入字符
    // 计算光标所在行
    // 计算光标所在列
    pub(crate) fn insert(&mut self, cursor_y: usize, cursor_x: usize, c: char) {
        let (line_index, line_offset) = self.calculate_x_y(cursor_y, cursor_x);
        let mut buf = [0u8; 4]; // 一个 char 最多需要 4 个字节存储 UTF-8 编码
        let s: &str = c.encode_utf8(&mut buf);
        let line = &mut self.lines[line_index];
        //如果line_offset大于文本长度 要填充空格
        if line_offset > line.text_len() {
            let gap_len = line_offset - line.text_len();
            line.insert(line.text_len(), " ".repeat(gap_len).as_str());
        }
        line.insert(line_offset, s);
    }

    // 插入换行
    pub(crate) fn insert_newline(&mut self, cursor_y: usize, cursor_x: usize) {
        let (line_index, line_offset) = self.calculate_x_y(cursor_y, cursor_x);
        //    println!("line_index: {}, line_offset: {}", line_index, line_offset);
        let line_txt = self.lines[line_index].text();
        let line_len = line_txt.len();
        {
            if line_offset > line_len {
                // 如果光标不在行尾，插入新行
                let new_gap_buffer = GapBuffer::new(10);
                self.lines.insert(line_index + 1, new_gap_buffer);
            } else {
                let b = &line_txt[line_offset..];
                let mut new_gap_buffer = GapBuffer::new(b.len() + 5);
                new_gap_buffer.insert(0, b);
                self.lines.insert(line_index + 1, new_gap_buffer);
            }
        }

        {
            if line_len > line_offset {
                let delete_len = line_len - line_offset;
                // 删除当前行的剩余部分
                self.lines[line_index].delete(line_len, delete_len);
            }
        }
    }

    // 删除光标前一个字符
    pub(crate) fn backspace(&mut self, cursor_y: usize, cursor_x: usize) {
        let (line_index, line_offset) = self.calculate_x_y(cursor_y, cursor_x);
        if self.lines[line_index].text().len() == 0 && line_offset == 0 {
            //删除一行
            self.lines.remove(line_index);
            return;
        }
        //表示当前行和前一行合并
        if line_offset == 0 {
            if line_index == 0 {
                return;
            }
            //用.split_at_mut(position)修改代码
            let (pre_lines, cur_lines) = self.lines.split_at_mut(line_index);
            let pre_line = &mut pre_lines[line_index - 1];
            if pre_line.text().len() == 0 {
                self.lines.remove(line_index - 1);
                return;
            } else {
                let cur_line = &mut cur_lines[0];
                let cur_line_txt = cur_line.text();
                pre_line.insert(pre_line.text_len(), cur_line_txt);
                self.lines.remove(line_index);
            }
            return;
        }
        self.lines[line_index].backspace(line_offset);
    }
    //从当前行开始获取后面n行
    pub(crate) fn get_next_line<'a>(
        &'a mut self,
        meta: &EditLineMeta,
        line_count: usize,
    ) -> (Option<&'a str>, EditLineMeta) {
        let mut line_index = meta.get_line_index();
        let mut line_end = meta.get_line_end();

        // if line_index >= ring.lines.len() {
        //     return (None, meta.clone());
        // }
        let line = &self.lines[line_index];
        if line_end == line.text_len() {
            line_end = 0;
            line_index += 1;
        }

        // let total_txt_len = self.rows.borrow_lines()[line_index].text_len();
        // //当前行没有读完 继续读
        // if total_txt_len == meta.get_line_end() {
        //     line_index += 1;
        //     line_end = 0;
        // }
        // println!(
        //     "total_txt_len:{},line_index:{},line_start:{}",
        //     total_txt_len, line_index, line_end
        // );
        let p = PageOffset {
            line_index: line_index,
            line_start: line_end,
        };
        let mut s = "";
        let mut m = EditLineMeta::default();
        let start_page_num = meta.get_line_num() / self.height;
        self.get_text_fn(
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
        (Some(s), m)
    }

    // 从第n行开始获取内容
    pub(crate) fn get_line_content<'a>(
        &'a mut self,
        line_num: usize,
        line_count: usize,
    ) -> (Vec<&'a str>, Vec<EditLineMeta>) {
        let mut split_lines = Vec::new();
        let mut line_meta_list: Vec<EditLineMeta> = Vec::new();
        self.get_text(line_num, line_count, |txt, meta| {
            split_lines.push(txt);
            line_meta_list.push(meta);
        });
        (split_lines, line_meta_list)
    }

    fn get_text<'a, F>(&'a mut self, line_num: usize, line_count: usize, mut f: F)
    where
        F: FnMut(&'a str, EditLineMeta),
    {
        let page_num = self.get_page_num(line_num);
        // 计算页码
        let index = (page_num - 1) / PAGE_GROUP;
        let page_offset = if index >= self.page_offset_list.len() {
            *self.page_offset_list.last().unwrap()
        } else {
            self.page_offset_list[index]
        };
        let start_page_num = index * PAGE_GROUP;
        assert!(line_num >= start_page_num * self.height);
        //跳过的行数
        let skip_line = line_num - start_page_num * self.height;

        self.get_text_fn(
            &page_offset,
            line_count,
            start_page_num * self.height,
            start_page_num,
            skip_line,
            &mut f,
        );
    }

    fn get_text_fn<'a, F>(
        &'a mut self,
        page_offset: &PageOffset,
        line_count: usize,
        start_line_num: usize,
        start_page_num: usize,
        skip_line: usize,
        f: &mut F,
    ) where
        // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
        F: FnMut(&'a str, EditLineMeta),
    {
        if page_offset.line_index >= self.lines.len() {
            return;
        }
        let (a, b) = self.lines.split_at_mut(page_offset.line_index + 1);
        let mut cur_line_count = 0;
        let mut line_num = start_line_num;
        let mut page_num = start_page_num;

        let line_txt = &a[page_offset.line_index].text()[page_offset.line_start..];
        Self::set_line_txt(
            line_txt,
            page_offset.line_index,
            page_offset.line_start,
            self.with,
            self.height,
            &mut self.page_offset_list,
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
        // 使用 split_at_mut 获取后续行的可变子切片
        for (i, line) in b.iter_mut().enumerate() {
            let line_txt = line.text();
            Self::set_line_txt(
                line_txt,
                page_offset.line_index + i + 1,
                0,
                self.with,
                self.height,
                &mut self.page_offset_list,
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

        // if page_offset.line_index >= self.lines.len() {
        //     return;
        // }

        //   for i in self.lines[page_offset.line_index..].iter() {}
    }

    fn set_line_txt<'a, F>(
        line_txt: &'a str,
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
        F: FnMut(&'a str, EditLineMeta),
    {
        //空行
        if line_txt.len() == 0 {
            *line_num += 1; //行数加1
            if *line_num >= skip_line {
                *cur_line_count += 1;
                f(
                    "",
                    EditLineMeta::new(0, *page_num + 1, *line_num, line_index, 0),
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
                        line_start: 0,
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

        for (byte_index, ch) in line_txt.char_indices() {
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
                            *page_num + 1,
                            *line_num,
                            line_index,
                            line_start + line_offset,
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
                            line_start: line_start + byte_index,
                        });
                    }
                }
                if *cur_line_count >= line_count {
                    return;
                }

                line_offset += current_bytes;
                current_width = 0;
                current_bytes = 0;
            }
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
                        *page_num + 1,
                        *line_num,
                        line_index,
                        line_start + line_offset,
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
                        line_start: 0,
                    });
                }
            }
            if *cur_line_count >= line_count {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::read_lines;

    #[test]
    fn test_print() {
        let mut b = EditTextBuffer::from_file_path("/root/aa.txt", 2, 5).unwrap();

        let c = {
            let (s, c) = b.get_line_content(1, 1);
            for (i, l) in s.iter().enumerate() {
                println!("l: {:?},{:?}", l, c[i]);
            }

            // for p in b.page_offset_list.iter() {
            //     println!("p:{:?}", p)
            // }
            c
        };
        let (s, m) = b.get_next_line(&c[0], 1);
        println!("{:?},{:?}", s, m);
        let (s, m) = b.get_next_line(&m, 1);
        println!("{:?},{:?}", s, m);
        let (s, m) = b.get_next_line(&m, 1);
        println!("{:?},{:?}", s, m);
        let (s, m) = b.get_next_line(&m, 1);
        println!("{:?},{:?}", s, m);
        let (s, m) = b.get_next_line(&m, 1);
        println!("{:?},{:?}", s, m);
        let (s, m) = b.get_next_line(&m, 1);
        println!("{:?},{:?}", s, m);
        let (s, m) = b.get_next_line(&m, 1);
        println!("{:?},{:?}", s, m);
        let (s, m) = b.get_next_line(&m, 1);
        println!("{:?},{:?}", s, m);
        let (s, m) = b.get_next_line(&m, 1);
        println!("{:?},{:?}", s, m);
        let (s, m) = b.get_next_line(&m, 1);
        println!("{:?},{:?}", s, m);
        let (s, m) = b.get_next_line(&m, 1);
        println!("{:?},{:?}", s, m);
        let (s, m) = b.get_next_line(&m, 1);
        println!("{:?},{:?}", s, m);
        let (s, m) = b.get_next_line(&m, 1);
        println!("{:?},{:?}", s, m);
        let (s, m) = b.get_next_line(&m, 1);
        println!("{:?},{:?}", s, m);
        let (s, m) = b.get_next_line(&m, 1);
        println!("{:?},{:?}", s, m);

        for p in b.page_offset_list.iter() {
            println!("p:{:?}", p)
        }
    }

    #[test]
    fn test_insert() {
        let mut b = EditTextBuffer::from_file_path("/root/aa.txt", 2, 5).unwrap();

        {
            let (s, c) = b.get_line_content(1, 10);
            for l in s {
                println!("l: {:?}", l);
            }
        }
        let y = 0;
        let x = 5;
        b.insert(y, x, 'a');
        // b.insert(y, x + 1, 'b');
        // b.insert(y, x + 1 + 1, 'c');
        // b.insert(y, x + 1 + 1 + 1, 'd');
        // b.insert(y, x + 1 + 1 + 1 + 1, 'e');
        {
            let (s, c) = b.get_line_content(1, 10);
            for l in s {
                println!("l1: {:?}", l);
            }
        }
    }

    #[test]
    fn test_insert_newline() {
        let mut b = EditTextBuffer::from_file_path("/root/aa.txt", 2, 5).unwrap();

        {
            let (s, c) = b.get_line_content(1, 10);
            for l in s {
                println!("l: {:?}", l);
            }
        }
        let cursor_y = 1;
        let cursor_x = 4;
        b.insert_newline(cursor_y, cursor_x);
        {
            let (s, c) = b.get_line_content(1, 10);
            for l in s {
                println!("l1: {:?}", l);
            }
        }
    }

    #[test]
    fn test_backspace() {
        let mut b = EditTextBuffer::from_file_path("/root/aa.txt", 2, 5).unwrap();

        {
            let (s, c) = b.get_line_content(1, 10);
            for l in s {
                println!("l: {:?}", l);
            }
        }
        let cursor_y = 6;
        let cursor_x = 0;
        b.backspace(cursor_y, cursor_x);
        {
            let (s, c) = b.get_line_content(1, 10);
            for l in s {
                println!("l1: {:?}", l);
            }
        }
    }

    #[test]
    fn test_calculate_lines() {
        let txt = "12345678910";
        let line_count = EditTextBuffer::calculate_lines(txt.len(), 5);
        println!("line_count: {}", line_count);
    }

    #[test]
    fn test_get_backup_name() {
        let filepath = "/root/aa/12345678910";
        let name = EditTextBuffer::get_backup_name(filepath).unwrap();
        println!("name: {:?}", name);
    }
}
