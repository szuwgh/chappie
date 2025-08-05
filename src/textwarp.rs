use std::fmt::Debug;

use memmap2::Mmap;
use unicode_width::UnicodeWidthChar;

use crate::fuzzy::{FuzzySearch, Match};

const PAGE_GROUP: usize = 1;

pub(crate) struct LineMeta {
    txt_len: usize,
    line_num: usize,
    end_offset: usize, //这一行在整个文件最后偏移量
    match_fuzzy: Option<Match>,
}

impl LineMeta {
    pub(crate) fn new(
        txt_len: usize,
        line_num: usize,
        end_offset: usize,
        match_fuzzy: Option<Match>,
    ) -> LineMeta {
        LineMeta {
            txt_len,
            line_num,
            end_offset: end_offset,
            match_fuzzy,
        }
    }

    pub(crate) fn get_line_num(&self) -> usize {
        self.line_num
    }

    pub(crate) fn get_match(&self) -> &Option<Match> {
        &self.match_fuzzy
    }

    pub(crate) fn get_txt_len(&self) -> usize {
        self.txt_len
    }
}

impl Debug for LineMeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{:5}", self.line_num))
    }
}

pub(crate) trait SimpleText {
    fn cursor(&self, offset: usize) -> &[u8];
    fn size(&self) -> usize;
    fn push_str(&mut self, msg: &str);
}

pub(crate) trait LineText {
    // fn
}

impl SimpleText for Mmap {
    fn cursor(&self, offset: usize) -> &[u8] {
        assert!(offset < self.size());
        &self[offset..]
    }

    fn size(&self) -> usize {
        self.len()
    }

    fn push_str(&mut self, msg: &str) {
        todo!()
    }
}

// pub(crate) struct SimpleString(String);

// impl SimpleString {
//     pub(crate) fn new(s: String) -> SimpleString {
//         SimpleString(s)
//     }
// }

// impl SimpleText for SimpleString {
//     fn cursor(&self, offset: usize) -> &[u8] {
//         assert!(self.size() == 0 || offset < self.size());
//         &self.0.as_bytes()[offset..]
//     }

//     fn size(&self) -> usize {
//         self.0.len()
//     }

//     fn push_str(&mut self, msg: &str) {
//         self.0.push_str(msg);
//     }
// }

impl SimpleText for String {
    fn cursor(&self, offset: usize) -> &[u8] {
        assert!(self.size() == 0 || offset < self.size());
        &self.as_bytes()[offset..]
    }

    fn size(&self) -> usize {
        self.len()
    }

    fn push_str(&mut self, msg: &str) {
        self.push_str(msg);
    }
}

struct TextHelper {
    page_offset_list: Vec<usize>,
    max_line_num: usize,
    max_page_num: usize,
    eof: bool,
}

pub(crate) struct SimpleTextEngine<T: SimpleText> {
    text: T,
    max_bytes: usize,
    height: usize,      //最大行数
    with: usize,        //最大列数
    helper: TextHelper, //文本辅助类
    fuzzy: FuzzySearch,
}

impl<T: SimpleText> SimpleTextEngine<T> {
    pub(crate) fn new(text: T, height: usize, with: usize) -> SimpleTextEngine<T> {
        assert!(height > 0);
        assert!(with > 0);

        SimpleTextEngine {
            max_bytes: text.size(),
            text: text,
            height: height, //最大行数
            with: with,     //最大列数
            helper: TextHelper {
                page_offset_list: vec![0],
                max_line_num: 0,
                max_page_num: 0,
                eof: false,
            },
            fuzzy: FuzzySearch::new(),
        }
    }

    pub(crate) fn warp_str(text: &str, with: usize) -> Vec<&str> {
        Self::warp(text.as_bytes(), with)
    }

    pub(crate) fn warp(text: &[u8], with: usize) -> Vec<&str> {
        let max_bytes = text.len();
        let mut split_lines = Vec::new();
        let mut start = 0;
        for (i, byte) in text.iter().enumerate() {
            if *byte == b'\n' || i == max_bytes.saturating_sub(1) {
                let line = &text[start..=i];
                let line_txt = std::str::from_utf8(line).unwrap();
                let mut current_width = 0; //最近
                let mut line_offset = 0; //
                let mut current_bytes = 0;
                for ch in line_txt.chars() {
                    let ch_width = ch.width().unwrap_or(0);
                    //检查是否超过屏幕宽度
                    if current_width + ch_width > with {
                        let end = (line_offset + current_bytes).min(line_txt.len());
                        let txt = &line_txt[line_offset..end];
                        split_lines.push(txt);
                        line_offset += current_bytes;
                        current_width = 0;
                        current_bytes = 0;
                    }
                    current_width += ch_width;
                    current_bytes += ch.len_utf8();
                }
                if current_bytes > 0 {
                    let txt = &line_txt[line_offset..];
                    split_lines.push(txt);
                    start = i + 1;
                }
            }
        }
        split_lines
    }

    pub(crate) fn push_str(&mut self, msg: &str) {
        self.text.push_str(msg);
        self.helper.max_line_num = 0;
        self.helper.max_page_num = 0;
        self.max_bytes = self.text.size();
        self.helper.eof = false;
    }

    fn get_page_num(&self, num: usize) -> usize {
        (num + self.height - 1) / self.height // 计算页码，等同于向上取整
    }

    pub(crate) fn get_max_scroll_num(&self) -> Option<usize> {
        if self.helper.eof {
            return Some(self.helper.max_line_num.saturating_sub(self.height));
        }
        None
    }

    pub(crate) fn get_start_end<'a>(
        &'a self,
        start: usize,
        end: usize,
    ) -> (Option<Vec<&'a str>>, Vec<LineMeta>) {
        self.get_line_content(start, end - start + 1, "", true)
    }

    fn get_line_with_fn_mut<'a, F>(
        &'a mut self,
        line_num: usize,
        line_count: usize,
        pattern: &str,
        is_exact: bool,
        f: F,
    ) where
        F: FnMut(&'a str, LineMeta),
    {
        assert!(line_num >= 1);
        //获取页数
        let page_num = self.get_page_num(line_num);
        let index = (page_num - 1) / PAGE_GROUP;
        let page_offset = if index >= self.helper.page_offset_list.len() {
            *self.helper.page_offset_list.last().unwrap()
        } else {
            self.helper.page_offset_list[index]
        };
        let start_page_num = index * PAGE_GROUP;
        if self.helper.eof && line_num > self.helper.max_line_num {
            return;
        }
        assert!(line_num >= start_page_num * self.height);
        //跳过的行数
        let skip_line = line_num - start_page_num * self.height;
        Self::get_text_fn(
            &self.text,
            page_offset,
            line_count,
            start_page_num,
            skip_line,
            Some(&mut self.helper),
            Some(&mut self.fuzzy),
            pattern,
            is_exact,
            self.height,
            self.with,
            f,
        );
    }

    fn get_line_with_fn<'a, F>(
        &'a self,
        line_num: usize,
        line_count: usize,
        pattern: &str,
        is_exact: bool,
        f: F,
    ) where
        F: FnMut(&'a str, LineMeta),
    {
        assert!(line_num >= 1);
        //获取页数
        let page_num = self.get_page_num(line_num);
        let index = (page_num - 1) / PAGE_GROUP;
        let page_offset = if index >= self.helper.page_offset_list.len() {
            *self.helper.page_offset_list.last().unwrap()
        } else {
            self.helper.page_offset_list[index]
        };
        let start_page_num = index * PAGE_GROUP;
        if self.helper.eof && line_num > self.helper.max_line_num {
            return;
        }
        assert!(line_num >= start_page_num * self.height);
        //跳过的行数
        let skip_line = line_num - start_page_num * self.height;
        Self::get_text_fn(
            &self.text,
            page_offset,
            line_count,
            start_page_num,
            skip_line,
            None,
            None,
            pattern,
            is_exact,
            self.height,
            self.with,
            f,
        );
    }

    fn get_text_fn<'a, F>(
        text: &'a T,
        page_offset: usize,
        line_count: usize,
        start_page_num: usize,
        skip_line: usize,
        mut page_offset_list: Option<&mut TextHelper>,
        mut fuzzy: Option<&mut FuzzySearch>,
        pattern: &str,
        is_exact: bool,
        height: usize,
        with: usize,
        mut f: F,
    ) where
        F: FnMut(&'a str, LineMeta),
    {
        //获取这行所在的页数
        let mmap = text.cursor(page_offset);
        let max_bytes = mmap.len();
        let mut cur_line_count = 0;
        let mut start = 0;
        let start_line_num = start_page_num * height;
        let mut line_num = 0;
        let mut page_num = start_page_num;
        let mut last_u8 = 0u8;
        for (i, byte) in mmap.iter().enumerate() {
            last_u8 = *byte;
            if *byte == b'\n' || i == max_bytes - 1 {
                let line = &mmap[start..=i];
                let line_txt = std::str::from_utf8(line).unwrap();
                let mut current_width = 0; //字符宽度
                let mut line_offset = 0; //
                let mut current_bytes = 0;
                for ch in line_txt.chars() {
                    let ch_width = ch.width().unwrap_or(0);
                    //检查是否超过屏幕宽度
                    if current_width + ch_width > with {
                        let end = (line_offset + current_bytes).min(line_txt.len());
                        line_num += 1; //行数加1
                        if line_num % height == 0 {
                            //到达一页
                            page_num += 1; //页数加1
                            if let Some(helper) = page_offset_list.as_deref_mut() {
                                let m = page_num / PAGE_GROUP;
                                let n = page_num % PAGE_GROUP;
                                if n == 0 && m > helper.page_offset_list.len() - 1 {
                                    //保存页数的偏移量
                                    helper.page_offset_list.push(page_offset + start + end);
                                }
                            }
                        }
                        if line_num >= skip_line {
                            let txt = &line_txt[line_offset..end];
                            if let Some(fuzzy) = fuzzy.as_deref_mut() {
                                if pattern.len() > 0 {
                                    let m = fuzzy.find(pattern, txt, is_exact);
                                    if m.is_match() {
                                        cur_line_count += 1;
                                        let end_offset = page_offset + start + end;
                                        f(
                                            txt,
                                            LineMeta::new(
                                                txt.len(),
                                                start_line_num + line_num,
                                                end_offset,
                                                Some(m),
                                            ),
                                        );
                                    }
                                }
                            } else {
                                cur_line_count += 1;
                                let end_offset = page_offset + start + end;
                                f(
                                    txt,
                                    LineMeta::new(
                                        txt.len(),
                                        start_line_num + line_num,
                                        end_offset,
                                        None,
                                    ),
                                );
                            }
                            if cur_line_count >= line_count {
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
                //如果当前行还有剩余
                if current_bytes > 0 {
                    line_num += 1;
                    if line_num % height == 0 {
                        page_num += 1; //页数加1
                        if let Some(helper) = page_offset_list.as_deref_mut() {
                            let m = page_num / PAGE_GROUP;
                            let n = page_num % PAGE_GROUP;
                            if n == 0 && m > helper.page_offset_list.len() - 1 {
                                //保存页数的偏移量
                                helper.page_offset_list.push(page_offset + i + 1);
                            }
                        }
                    }
                    if line_num >= skip_line {
                        let txt = &line_txt[line_offset..];
                        if let Some(fuzzy) = fuzzy.as_deref_mut() {
                            if pattern.len() > 0 {
                                let m = fuzzy.find(pattern, txt, is_exact);
                                if m.is_match() {
                                    cur_line_count += 1;
                                    let end_offset = page_offset + i + 1;
                                    f(
                                        txt,
                                        LineMeta::new(
                                            txt.len(),
                                            start_line_num + line_num,
                                            end_offset,
                                            Some(m),
                                        ),
                                    );
                                }
                            }
                        } else {
                            cur_line_count += 1;
                            let end_offset = page_offset + i + 1;
                            f(
                                txt,
                                LineMeta::new(
                                    txt.len(),
                                    start_line_num + line_num,
                                    end_offset,
                                    None,
                                ),
                            );
                        }
                        if cur_line_count >= line_count {
                            return;
                        }
                    }
                }
                start = i + 1;
            }
        }

        if last_u8 == b'\n' {
            line_num += 1;
            if line_num % height == 0 {
                page_num += 1; //页数加1
                if let Some(helper) = page_offset_list.as_deref_mut() {
                    let m = page_num / PAGE_GROUP;
                    let n = page_num % PAGE_GROUP;
                    if n == 0 && m > helper.page_offset_list.len() - 1 {
                        //保存页数的偏移量
                        helper.page_offset_list.push(page_offset + start);
                    }
                }
            }
        }

        //没有剩余
        if cur_line_count < line_count {
            if let Some(helper) = page_offset_list {
                helper.max_line_num = start_line_num + line_num;
                helper.max_page_num = page_num;
                helper.eof = true;
            }
        }
    }

    fn get_line_content<'a>(
        &'a self,
        line_num: usize,
        line_count: usize,
        pattern: &str,
        is_exact: bool,
    ) -> (Option<Vec<&'a str>>, Vec<LineMeta>) {
        let mut split_lines = Vec::new();
        let mut line_meta_list = Vec::new();
        self.get_line_with_fn(line_num, line_count, pattern, is_exact, |txt, meta| {
            split_lines.push(txt);
            line_meta_list.push(meta);
        });
        if split_lines.len() > 0 {
            (Some(split_lines), line_meta_list)
        } else {
            (None, line_meta_list)
        }
    }

    pub(crate) fn get_line_count(&mut self) -> usize {
        if self.helper.eof {
            return self.helper.max_line_num;
        }

        let page_offset = if let Some(offset) = self.helper.page_offset_list.last() {
            *offset
        } else {
            0
        };
        let start_page_num = (self.helper.page_offset_list.len() - 1) * PAGE_GROUP;

        Self::get_text_fn(
            &self.text,
            page_offset,
            usize::MAX,
            start_page_num,
            0,
            Some(&mut self.helper),
            Some(&mut self.fuzzy),
            "",
            false,
            self.height,
            self.with,
            |_, _| {},
        );
        self.helper.max_line_num
    }

    // fn get_line_with_count<'a>(
    //     &'a mut self,
    //     line_num: usize,
    //     line_count: usize,
    //     pattern: &str,
    //     is_exact: bool,
    // ) -> (Option<Vec<&'a str>>, Vec<LineMeta>) {
    //     assert!(line_num >= 1);
    //     //获取页数
    //     let page_num = self.get_page_num(line_num);
    //     let index = (page_num - 1) / PAGE_GROUP;
    //     let page_offset = if index >= self.page_offset_list.len() {
    //         *self.page_offset_list.last().unwrap()
    //     } else {
    //         self.page_offset_list[index]
    //     };
    //     let start_page_num = index * PAGE_GROUP;
    //     if self.eof && line_num > self.max_line_num {
    //         return (None, Vec::new());
    //     }
    //     assert!(line_num >= start_page_num * self.height);
    //     //的行数
    //     let skip_line = line_num - start_page_num * self.height;

    //     self.get_text(
    //         page_offset,
    //         line_count,
    //         start_page_num,
    //         skip_line,
    //         pattern,
    //         is_exact,
    //     )
    // }

    pub(crate) fn get_line<'a>(
        &'a mut self,
        line_num: usize,
        pattern: &str,
        is_exact: bool,
    ) -> (Option<Vec<&'a str>>, Vec<LineMeta>) {
        self.get_line_content(line_num, self.height, pattern, is_exact)
    }

    // // 获取文本
    // fn get_text<'a>(
    //     &'a mut self,
    //     page_offset: usize,
    //     line_count: usize,
    //     start_page_num: usize,
    //     skip_line: usize,
    //     pattern: &str,
    //     is_exact: bool,
    // ) -> (Option<Vec<&'a str>>, Vec<LineMeta>) {
    //     //获取这行所在的页数
    //     let mmap = self.text.cursor(page_offset);
    //     let max_bytes = mmap.len();
    //     let mut split_lines = Vec::new();
    //     let mut line_meta_list = Vec::new();
    //     let mut start = 0;
    //     let start_line_num = start_page_num * self.height;
    //     let mut line_num = 0;
    //     let mut page_num = start_page_num;
    //     let mut last_u8 = 0u8;
    //     for (i, byte) in mmap.iter().enumerate() {
    //         last_u8 = *byte;
    //         if *byte == b'\n' || i == max_bytes - 1 {
    //             let line = &mmap[start..=i];
    //             let line_txt = std::str::from_utf8(line).unwrap();
    //             let mut current_width = 0; //
    //             let mut line_offset = 0; //
    //             let mut current_bytes = 0;
    //             for ch in line_txt.chars() {
    //                 let ch_width = ch.width().unwrap_or(0);
    //                 //检查是否超过屏幕宽度
    //                 if current_width + ch_width > self.with {
    //                     let end = (line_offset + current_bytes).min(line_txt.len());
    //                     line_num += 1; //行数加1
    //                     if line_num % self.height == 0 {
    //                         //到达一页
    //                         page_num += 1; //页数加1
    //                         let m = page_num / PAGE_GROUP;
    //                         let n = page_num % PAGE_GROUP;
    //                         if n == 0 && m > self.page_offset_list.len() - 1 {
    //                             //保存页数的偏移量
    //                             self.page_offset_list.push(page_offset + start + end);
    //                         }
    //                     }
    //                     if line_num >= skip_line {
    //                         let txt = &line_txt[line_offset..end];
    //                         if pattern.len() > 0 {
    //                             let m = self.fuzzy.find(pattern, txt, is_exact);
    //                             if m.is_match() {
    //                                 split_lines.push(txt);
    //                                 line_meta_list
    //                                     .push(LineMeta::new(start_line_num + line_num, Some(m)));
    //                             }
    //                         } else {
    //                             split_lines.push(txt);
    //                             line_meta_list.push(LineMeta::new(start_line_num + line_num, None));
    //                         }
    //                         if split_lines.len() >= line_count {
    //                             return (Some(split_lines), line_meta_list);
    //                         }
    //                     }

    //                     line_offset += current_bytes;
    //                     current_width = 0;
    //                     current_bytes = 0;
    //                 }
    //                 current_width += ch_width;
    //                 current_bytes += ch.len_utf8();
    //             }
    //             if current_bytes > 0 {
    //                 line_num += 1;
    //                 if line_num % self.height == 0 {
    //                     page_num += 1; //页数加1
    //                     let m = page_num / PAGE_GROUP;
    //                     let n = page_num % PAGE_GROUP;
    //                     if n == 0 && m > self.page_offset_list.len() - 1 {
    //                         //保存页数的偏移量
    //                         self.page_offset_list.push(page_offset + i + 1);
    //                     }
    //                 }
    //                 if line_num >= skip_line {
    //                     let txt = &line_txt[line_offset..];
    //                     if pattern.len() > 0 {
    //                         let m = self.fuzzy.find(pattern, txt, is_exact);
    //                         if m.is_match() {
    //                             split_lines.push(txt);
    //                             line_meta_list
    //                                 .push(LineMeta::new(start_line_num + line_num, Some(m)));
    //                         }
    //                     } else {
    //                         split_lines.push(txt);
    //                         line_meta_list.push(LineMeta::new(start_line_num + line_num, None));
    //                     }
    //                     if split_lines.len() >= line_count {
    //                         return (Some(split_lines), line_meta_list);
    //                     }
    //                 }
    //             }
    //             start = i + 1;
    //         }
    //     }

    //     if last_u8 == b'\n' {
    //         line_num += 1;
    //         if line_num % self.height == 0 {
    //             page_num += 1; //页数加1
    //             let m = page_num / PAGE_GROUP;
    //             let n = page_num % PAGE_GROUP;
    //             if n == 0 && m > self.page_offset_list.len() - 1 {
    //                 //保存页数的偏移量
    //                 self.page_offset_list.push(page_offset + start);
    //             }
    //         }
    //     }
    //     //没有剩余
    //     if split_lines.len() < line_count {
    //         self.max_line_num = start_line_num + line_num;
    //         self.max_page_num = page_num;
    //         self.eof = true;
    //     }
    //     if split_lines.len() > 0 {
    //         (Some(split_lines), line_meta_list)
    //     } else {
    //         (None, line_meta_list)
    //     }
    // }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::mmap_file;
    use std::io;

    #[test]
    fn test_next_line1() -> io::Result<()> {
        let file_path = "/opt/rsproject/chappie/vectorbase/src/disk.rs";
        let mmap = mmap_file(file_path)?;
        // let (navi, visible_content, length) = get_visible_content(&mmap, 0, 30, 5, "");
        // println!("{},{}", visible_content, length);
        // Ok(())

        let mut eg = SimpleTextEngine::new(mmap, 37, 75);

        for i in (1..=1800).step_by(37) {
            println!("--------{}---------", i);
            if let (_, meta) = eg.get_line(i, "", true) {
                for (i, v) in meta.into_iter().enumerate() {
                    println!("{:?}", v);
                }
            }
            println!("{:?}", eg.get_max_scroll_num())
        }
        println!("--------{}---------", 1666);
        if let (_, meta) = eg.get_line(1666, "", true) {
            for (i, v) in meta.into_iter().enumerate() {
                println!("{:?}", v);
            }
        }
        println!("{:?}", eg.get_max_scroll_num());
        Ok(())
    }

    #[test]
    fn test_get_last() -> io::Result<()> {
        let file_path = "/opt/rsproject/chappie/crates/vectorbase/src/disk.rs";
        let mmap = mmap_file(file_path)?;
        // let (navi, visible_content, length) = get_visible_content(&mmap, 0, 30, 5, "");
        // println!("{},{}", visible_content, length);
        // Ok(())

        let mut eg = SimpleTextEngine::new(mmap, 37, 10000);
        eg.get_line(100, "", true);
        println!("last_line:{}", eg.get_line_count());
        if let (_, a1) = eg.get_line(1594 + 1, "", true) {
            for v in a1.into_iter() {
                println!("{:?}", v);
            }
        }
        Ok(())
    }

    #[test]
    fn test_get_start_end() -> io::Result<()> {
        let file_path = "/opt/rsproject/chappie/vectorbase/src/disk.rs";
        let mmap = mmap_file(file_path)?;
        // let (navi, visible_content, length) = get_visible_content(&mmap, 0, 30, 5, "");
        // println!("{},{}", visible_content, length);
        // Ok(())

        let mut eg = SimpleTextEngine::new(mmap, 37, 10);

        if let (_, a1) = eg.get_start_end(3, 96) {
            for v in a1.into_iter() {
                println!("{:?}", v);
            }
        }
        Ok(())
    }

    #[test]
    fn test_get_line_count2() -> io::Result<()> {
        let file_path = "/root/aa.txt";
        let mmap = mmap_file(file_path)?;
        // let (navi, visible_content, length) = get_visible_content(&mmap, 0, 30, 5, "");
        // println!("{},{}", visible_content, length);
        // Ok(())

        let mut eg = SimpleTextEngine::new(mmap, 37, 5);

        let (txt) = eg.get_line_content(1, 10, "", false);
        // if let Some(c) = txt {
        //     for (v, l) in c.into_iter().zip(meta.into_iter()) {
        //         println!("{:?},{:?}", v, l.end_offset);
        //     }
        // }
        Ok(())
    }

    #[test]
    fn test_string() -> io::Result<()> {
        let mut eg = SimpleTextEngine::new(String::with_capacity(10), 3, 3);

        let a = "12345\n22345\n32345\n";
        eg.push_str(a);
        let num = eg.get_line_count();
        println!("{}", num);

        let num = eg.get_line_count();
        println!("{}", num);
        let b = "42345\n52345\n62399\n";
        eg.push_str(b);
        let num = eg.get_line_count();
        println!("{}", num);

        let a = "12345\n22345\n32345\n";
        eg.push_str(a);
        let num = eg.get_line_count();
        println!("{}", num);

        let (line) = eg.get_line(8, "", false);
        println!("{:?}", line);

        let (line) = eg.get_line(11, "", false);
        println!("{:?}", line);
        println!("page_offset:{:?}", eg.helper.page_offset_list);

        Ok(())
    }
}
