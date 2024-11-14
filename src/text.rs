use std::fmt::{Debug, Write};

use unicode_width::UnicodeWidthChar;

use crate::fuzzy::fuzzy_search;

//每十页建立一个索引
const PAGE_GROUP: usize = 10;

pub(crate) struct LineTxt<'a> {
    text: &'a str,
    line_num: usize,
}

impl<'a> LineTxt<'a> {
    fn new(text: &'a str, line_num: usize) -> LineTxt {
        LineTxt { text, line_num }
    }

    pub(crate) fn get_text(&self) -> &'a str {
        self.text
    }

    pub(crate) fn get_line_num(&self) -> usize {
        self.line_num
    }
}

impl<'a> Debug for LineTxt<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{:5} {}", self.line_num, self.text))
    }
}

pub(crate) struct SimpleTextEngine<'a> {
    text: &'a [u8],
    height: usize, //最大行数
    with: usize,   //最大列数
    page_offset_list: Vec<usize>,
    max_line_num: usize,
    max_page_num: usize,
    eof: bool,
}

impl<'a> SimpleTextEngine<'a> {
    pub(crate) fn new(text: &'a [u8], height: usize, with: usize) -> SimpleTextEngine<'a> {
        SimpleTextEngine {
            text: text,
            height: height, //最大行数
            with: with,     //最大列数
            // next_line_offset: 0, //下一行的偏移量
            page_offset_list: vec![0],
            max_line_num: 0,
            max_page_num: 0,
            eof: false,
        }
    }

    fn get_page_num(&mut self, num: usize) -> usize {
        (num + self.height - 1) / self.height // 计算页码，等同于向上取整
    }

    pub(crate) fn get_max_scroll_num(&self) -> Option<usize> {
        if self.eof {
            return Some(self.max_line_num - self.height);
        }
        None
    }

    // pub(crate) fn get_page(&mut self, page_num: usize) -> Option<Vec<&'a str>> {
    //     let line_num = page_num * self.height;
    //     self.get_line(line_num)
    // }

    pub(crate) fn get_line(&mut self, line_num: usize, pattern: &str) -> Option<Vec<LineTxt<'a>>> {
        assert!(line_num >= 1);
        //获取页数
        let page_num = self.get_page_num(line_num);
        let index = (page_num - 1) / PAGE_GROUP;
        let page_offset = if index >= self.page_offset_list.len() {
            *self.page_offset_list.last().unwrap()
        } else {
            self.page_offset_list[index]
        };
        let start_page_num = index * PAGE_GROUP;
        if self.eof && line_num > self.max_line_num {
            return None;
        }
        assert!(line_num >= start_page_num * self.height);
        //剩余的行数
        let skip_line = line_num - start_page_num * self.height;

        self.get_text(page_offset, start_page_num, skip_line, pattern)
    }

    // 获取文本
    fn get_text(
        &mut self,
        page_offset: usize,
        start_page_num: usize,
        skip_line: usize,
        pattern: &str,
    ) -> Option<Vec<LineTxt<'a>>> {
        // if offset >= self.text.len() {
        //     return None;
        // }
        //获取这行所在的页数

        let mmap = &self.text[page_offset..];
        let mut split_lines = Vec::new();
        let mut start = 0;
        let start_line_num = start_page_num * self.height;
        let mut line_num = 0;
        let mut page_num = start_page_num;
        for (i, byte) in mmap.iter().enumerate() {
            if *byte == b'\n' {
                let line = &mmap[start..=i];
                let line_txt = std::str::from_utf8(line).unwrap();
                let mut current_width = 0;
                let mut line_offset = 0;
                let mut current_bytes = 0;
                for ch in line_txt.chars() {
                    let ch_width = ch.width().unwrap_or(0);
                    //检查是否超过屏幕宽度
                    if current_width + ch_width > self.with {
                        let end = (line_offset + current_bytes).min(line_txt.len());
                        line_num += 1; //行数加1
                        if line_num % self.height == 0 {
                            //到达一页
                            page_num += 1; //页数加1
                            let m = page_num / PAGE_GROUP;
                            let n = page_num % PAGE_GROUP;
                            if n == 0 && m > self.page_offset_list.len() - 1 {
                                //保存页数的偏移量
                                self.page_offset_list.push(page_offset + end + 1);
                            }
                        }
                        if line_num < skip_line {
                        } else {
                            let txt = &line_txt[line_offset..end];
                            if pattern.len() > 0 {
                                let m = fuzzy_search(pattern, txt, true);
                                if m.is_match() {
                                    split_lines.push(LineTxt::new(txt, start_line_num + line_num));
                                }
                            } else {
                                split_lines.push(LineTxt::new(txt, start_line_num + line_num));
                            }
                            if split_lines.len() >= self.height {
                                return Some(split_lines);
                            }
                        }

                        line_offset += current_bytes;
                        current_width = 0;
                        current_bytes = 0;
                    }
                    current_width += ch_width;
                    current_bytes += ch.len_utf8();
                }
                if current_bytes > 0 {
                    line_num += 1;
                    if line_num % self.height == 0 {
                        page_num += 1; //页数加1
                        let m = page_num / PAGE_GROUP;
                        let n = page_num % PAGE_GROUP;
                        if n == 0 && m > self.page_offset_list.len() - 1 {
                            //保存页数的偏移量
                            self.page_offset_list.push(page_offset + i + 1);
                        }
                    }
                    if line_num < skip_line {
                    } else {
                        let txt = &line_txt[line_offset..];
                        if pattern.len() > 0 {
                            let m = fuzzy_search(pattern, txt, true);
                            if m.is_match() {
                                split_lines.push(LineTxt::new(txt, start_line_num + line_num));
                            }
                        } else {
                            split_lines.push(LineTxt::new(txt, start_line_num + line_num));
                        }
                        if split_lines.len() >= self.height {
                            return Some(split_lines);
                        }
                    }
                }
                start = i + 1;
            }
        }
        //没有剩余
        if split_lines.len() < self.height {
            if !self.eof {
                self.max_line_num = start_line_num + line_num + 1;
                self.max_page_num = page_num;
                self.eof = true;
            }
        }
        if split_lines.len() > 0 {
            Some(split_lines)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::map_file;
    use std::io;

    #[test]
    fn test_next_line1() -> io::Result<()> {
        let file_path = "/opt/rsproject/chappie/vectorbase/src/disk.rs";
        let mmap = map_file(file_path)?;
        // let (navi, visible_content, length) = get_visible_content(&mmap, 0, 30, 5, "");
        // println!("{},{}", visible_content, length);
        // Ok(())

        let mut eg = SimpleTextEngine::new(&mmap, 37, 75);

        for i in (1..=1800).step_by(37) {
            println!("--------{}---------", i);
            if let Some(a1) = eg.get_line(i, "") {
                for v in a1.into_iter() {
                    println!("{:?}", v);
                }
            }
            println!("{:?}", eg.get_max_scroll_num())
        }
        println!("--------{}---------", 1666);
        if let Some(a1) = eg.get_line(1666, "") {
            for v in a1.into_iter() {
                println!("{:?}", v);
            }
        }
        println!("{:?}", eg.get_max_scroll_num());
        Ok(())
    }

    // #[test]
    // fn test_next_page2() -> io::Result<()> {
    //     let file_path = "/opt/rsproject/chappie/vectorbase/src/disk.rs";
    //     let mmap = map_file(file_path)?;
    //     // let (navi, visible_content, length) = get_visible_content(&mmap, 0, 30, 5, "");
    //     // println!("{},{}", visible_content, length);
    //     // Ok(())

    //     let mut eg = SimpleTextEngine::new(&mmap, 37, 75);
    //     for i in 1..=50 {
    //         println!("--------{}---------", i);
    //         if let Some(a1) = eg.get_page(i, "") {
    //             for v in a1.into_iter() {
    //                 println!("{:?}", v);
    //             }
    //         }
    //         println!("{:?}", eg.get_max_scroll_num())
    //     }
    //     Ok(())
    // }
}
