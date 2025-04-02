use crate::util::read_lines;
use crate::{error::ChapResult, gap_buffer::GapBuffer};
use ratatui::symbols::line;
use std::path::Path;
use unicode_width::UnicodeWidthChar;
const PAGE_GROUP: usize = 1;
use crate::text::LineMeta;
pub(crate) struct EditTextBuffer {
    lines: Vec<GapBuffer>, // 每行使用 GapBuffer 存储
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

// struct LineMeta {
//     line_num: usize,   // 行号
//     line_start: usize, // 行的起始位置
//     line_end: usize,   // 行的结束位置
// }

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

    pub(crate) fn get_line_content<'a>(
        &'a mut self,
        line_num: usize,
        line_count: usize,
    ) -> (Vec<&'a str>, Vec<LineMeta>) {
        let mut split_lines = Vec::new();
        let mut line_meta_list = Vec::new();
        self.get_text(line_num, line_count, |txt, meta| {
            split_lines.push(txt);
            line_meta_list.push(meta);
        });
        (split_lines, line_meta_list)
    }

    fn get_text<'a, F>(&'a mut self, line_num: usize, line_count: usize, mut f: F)
    where
        F: FnMut(&'a str, LineMeta),
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

        self.get_text_fn(&page_offset, line_count, start_page_num, skip_line, &mut f);
    }

    fn get_text_fn<'a, F>(
        &'a mut self,
        page_offset: &PageOffset,
        line_count: usize,
        start_page_num: usize,
        skip_line: usize,
        f: &mut F,
    ) where
        // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
        F: FnMut(&'a str, LineMeta),
    {
        if page_offset.line_index >= self.lines.len() {
            return;
        }
        let (a, b) = self.lines.split_at_mut(page_offset.line_index + 1);
        let mut line_num = 0;
        let mut cur_line_count = 0;
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
        F: FnMut(&'a str, LineMeta),
    {
        //空行
        if line_txt.len() == 0 {
            *line_num += 1; //行数加1
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
            if *line_num >= skip_line {
                f("", LineMeta::new(0, *line_num, 0, None));
                if *cur_line_count >= line_count {
                    return;
                }
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
                if *line_num % height == 0 {
                    //到达一页
                    *page_num += 1; //页数加1
                    let m = *page_num / PAGE_GROUP;
                    let n = *page_num % PAGE_GROUP;
                    if n == 0 && m > page_offset_list.len() - 1 {
                        //保存页数的偏移量
                        page_offset_list.push(PageOffset {
                            line_index,
                            line_start: byte_index,
                        });
                    }
                }
                if *line_num >= skip_line {
                    let txt = &line_txt[line_offset..end];
                    f(
                        txt,
                        LineMeta::new(txt.len(), *line_num, line_start + line_offset, None),
                    );
                    if *cur_line_count >= line_count {
                        return;
                    }
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
            if *line_num >= skip_line {
                let txt = &line_txt[line_offset..];
                *cur_line_count += 1;
                f(
                    txt,
                    LineMeta::new(txt.len(), *line_num, line_start + line_offset, None),
                );
                if *cur_line_count >= line_count {
                    return;
                }
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

        {
            let (s, c) = b.get_line_content(1, 10);
            for l in s {
                println!("l: {:?}", l);
            }
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
}
