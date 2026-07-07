use crate::common::util::mmap_file;
use crate::searcher::memchr::memchr;
use crate::textwarp::ChapResult;
use crate::textwarp::LineData;
use crate::textwarp::LineState;
use crate::textwarp::LineStr;
use crate::textwarp::Path;
use crate::textwarp::Text;
use crate::textwarp::TextIndex;
use crate::textwarp::TextSelect;
use memmap2::Mmap;
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
    file_size: usize,
    /// 首行已知的 line_file_end（None 表示不知道，需要扫描）
    first_line_file_end: Option<usize>,
}

impl<'a> Iterator for MmapTextIter<'a> {
    type Item = LineStr<'a>;
    fn next(&mut self) -> Option<LineStr<'a>> {
        if self.line_file_start >= self.file_size {
            return None;
        }
        let slice = &self.mmap[self.line_file_start..];

        // 优化 1: 首行已知 line_file_end，无需扫描 \n
        let end = if let Some(known) = self.first_line_file_end.take() {
            if known > self.line_file_start {
                known - self.line_file_start
            } else {
                // 优化 2: SIMD 扫描 \n（比逐字节快 10-20 倍）
                memchr(slice, b'\n').unwrap_or(slice.len().saturating_sub(1))
            }
        } else {
            memchr(slice, b'\n').unwrap_or(slice.len().saturating_sub(1))
        };

        let line = &slice[..end];
        let line_start = self.line_file_start;
        self.line_file_start += end + 1;
        Some(LineStr {
            data: LineData::Bytes(line),
            block_id: 0,
            block_offset: 0,
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
        file_size: usize,
        first_line_file_end: Option<usize>,
    ) -> MmapTextIter<'a> {
        MmapTextIter {
            mmap,
            line_index,
            line_file_start,
            file_size,
            first_line_file_end,
        }
    }
}

impl TextIndex for MmapText {
    fn get_page_offset(&self, line_num: usize) -> LineState {
        LineState::builder().build()
    }

    fn set_page_offset(&mut self, page_num: usize, page_offset: LineState) {
        // MmapText 不支持分页偏移
    }
}

impl Text for MmapText {
    type LineItem<'a> = LineStr<'a>;
    fn get_file_size(&self) -> usize {
        self.mmap.len()
    }

    fn text_from_sel(&self, sel: &TextSelect) -> Vec<u8> {
        todo!("Not implement text_from_sel for MmapText");
    }

    fn get_line<'a>(&'a self, state: &LineState) -> Option<LineStr<'a>> {
        let line = &self.mmap[state.line_file_start + state.line_offset..state.line_file_end];
        Some(LineStr {
            data: LineData::Bytes(line),
            block_id: state.block_num,
            block_offset: state.block_offset,
            line_file_start: state.line_file_start,
            line_file_end: state.line_file_end,
        })
    }

    fn has_pre_line(&self, meta: &LineState) -> bool {
        if meta.get_line_file_start() == 0 && meta.get_line_offset() == 0 {
            return false;
        }
        true
    }

    fn get_next_line_state(&self, state: &LineState) -> Option<LineState> {
        // 逻辑行总长度（line_file_end 指向 \n，不含 \n）
        let total_len = state.line_file_end.saturating_sub(state.line_file_start);
        // 已消费的字节数（当前视觉子行在逻辑行内的偏移 + 子行长度）
        let consumed = state.line_offset + state.txt_len;

        let p = if consumed >= total_len {
            // 逻辑行已全部消费 → 跳到下一个逻辑行（+1 跳过 \n）
            // 注意：line_file_end 不设置，因为不知道下一行的结束位置
            LineState::builder()
                .line_index(state.line_index + 1)
                .line_file_start(state.line_file_end + 1)
                .start_line_num(state.get_line_num())
                .build()
        } else {
            // SoftWrap 中间子行 → 从 consumed 处继续当前逻辑行
            LineState::builder()
                .line_index(state.line_index)
                .line_offset(consumed)
                .line_file_start(state.line_file_start)
                .line_file_end(state.line_file_end) // 同一逻辑行，line_file_end 已知
                .start_line_num(state.get_line_num())
                .build()
        };
        Some(p)
    }

    fn get_pre_line_state(&self, state: &LineState) -> Option<LineState> {
        // 已经是文件第一行（第一个逻辑行的第一个子行）
        if state.line_file_start == 0 && state.line_offset == 0 {
            return None;
        }

        if state.line_offset > 0 {
            // SoftWrap 中间/最后子行 → 同一逻辑行内反向，sort_warp_desc 处理 line_start 之前的文本
            Some(
                LineState::builder()
                    .line_index(state.line_index)
                    .line_file_start(state.line_file_start)
                    .line_file_end(state.line_file_end)
                    .line_offset(state.line_offset) // 保留 offset，让 sort_warp_desc 从此处反向切
                    .start_line_num(state.get_line_num())
                    .build(),
            )
        } else {
            // 逻辑行第一个子行 → 找上一个逻辑行（反向扫 \n）
            let prev_end = state.line_file_start.saturating_sub(1);
            let prev_start = if prev_end == 0 {
                0
            } else {
                self.mmap[..prev_end]
                    .iter()
                    .rposition(|&b| b == b'\n')
                    .map(|p| p + 1) // \n 的下一个字节
                    .unwrap_or(0) // 没找到就是文件头
            };

            Some(
                LineState::builder()
                    .line_index(state.line_index.saturating_sub(1))
                    .line_file_start(prev_start)
                    .line_file_end(prev_end)
                    .start_line_num(state.get_line_num())
                    .build(),
            )
        }
    }

    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize {
        line_end - line_start
    }

    fn has_next_line(&self, state: &LineState) -> bool {
        if state.get_line_file_start() + state.get_line_offset() + state.get_txt_len()
            >= self.mmap.len() - 1
        {
            return false;
        }
        true
    }

    fn iter<'a>(&'a mut self, line_state: &LineState) -> impl Iterator<Item = LineStr<'a>> {
        let known_end = (line_state.line_file_end > 0).then_some(line_state.line_file_end);
        MmapTextIter::new(
            &self.mmap,
            line_state.line_index,
            line_state.line_file_start,
            self.mmap.len(),
            known_end,
        )
    }

    fn iter_rev<'a>(&'a mut self, line_state: &LineState) -> impl Iterator<Item = LineStr<'a>> {
        self.iter(line_state)
    }

    fn iter_u8<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_file_start: usize,
    ) -> impl Iterator<Item = u8> {
        Vec::new().into_iter() // MmapText 不支持 u8 迭代
    }

    fn search(&mut self, partten: &[u8], state: &LineState) -> ChapResult<Option<Vec<LineState>>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 从 MmapTextIter 产出的 LineStr 中提取原始字节
    fn extract_bytes(line: &LineStr) -> Vec<u8> {
        match &line.data {
            LineData::Bytes(b) => b.to_vec(),
            LineData::GapBytes(gb) => {
                let (l, r) = gb.as_slice();
                let mut v = Vec::with_capacity(l.len() + r.len());
                v.extend_from_slice(l);
                v.extend_from_slice(r);
                v
            }
            other => panic!("MmapText 产出类型异常: {:?}", other),
        }
    }

    #[test]
    fn test_mmap_text_iter_all_lines() {
        let mut mmap_text =
            MmapText::from_file_path("/home/unvdb/a.txt").expect("打开 /home/unvdb/a.txt 失败");
        let file_size = mmap_text.get_file_size();
        assert!(file_size > 0, "文件不应为空");

        // 用 std 读取文件并按 \n 切分，作为期望值
        let raw = std::fs::read("/home/unvdb/a.txt").unwrap();
        let split_all: Vec<&[u8]> = raw.split(|b| *b == b'\n').collect();
        // 文件以 \n 结尾时，split 末尾多一个空元素；迭代器不会产出该空行
        let expected_lines: &[&[u8]] = if raw.last() == Some(&b'\n') {
            &split_all[..split_all.len() - 1]
        } else {
            &split_all[..]
        };

        let line_state = LineState::builder()
            .line_index(0)
            .line_file_start(0)
            .build();

        let actual: Vec<LineStr> = mmap_text.iter(&line_state).collect();

        assert_eq!(
            actual.len(),
            expected_lines.len(),
            "行数不匹配: 期望 {}, 实际 {}",
            expected_lines.len(),
            actual.len()
        );

        let mut expected_file_start = 0usize;
        for (i, (line, exp_bytes)) in actual.iter().zip(expected_lines.iter()).enumerate() {
            let bytes = extract_bytes(line);

            // 内容一致
            assert_eq!(
                bytes,
                *exp_bytes,
                "第 {} 行内容不匹配\n期望: {:?}\n实际: {:?}",
                i,
                String::from_utf8_lossy(exp_bytes),
                String::from_utf8_lossy(&bytes),
            );

            // 位置信息正确
            assert_eq!(
                line.line_file_start, expected_file_start,
                "第 {} 行 line_file_start 应为 {}，实际 {}",
                i, expected_file_start, line.line_file_start
            );
            let expected_end = expected_file_start + exp_bytes.len();
            assert_eq!(
                line.line_file_end, expected_end,
                "第 {} 行 line_file_end 应为 {}，实际 {}",
                i, expected_end, line.line_file_end
            );

            expected_file_start = line.line_file_end + 1; // +1 跳过 \n
        }

        // 最后一行不能超出文件末尾
        if let Some(last) = actual.last() {
            assert!(last.line_file_end <= file_size);
        }
    }

    #[test]
    fn test_mmap_text_iter_from_middle() {
        // 从第二行开始迭代
        let mut mmap_text =
            MmapText::from_file_path("/home/unvdb/a.txt").expect("打开 /home/unvdb/a.txt 失败");
        let raw = std::fs::read("/home/unvdb/a.txt").unwrap();
        let first_nl = raw
            .iter()
            .position(|b| *b == b'\n')
            .expect("文件至少有一行");
        let second_line_start = first_nl + 1;

        let line_state = LineState::builder()
            .line_index(1)
            .line_file_start(second_line_start)
            .build();

        let lines: Vec<LineStr> = mmap_text.iter(&line_state).collect();
        assert!(!lines.is_empty(), "第二行开始应该有数据");

        // 第一条产出行的 line_file_start 应该等于我们设置的起始位置
        assert_eq!(
            lines[0].line_file_start, second_line_start,
            "首行应从 {} 开始，实际从 {} 开始",
            second_line_start, lines[0].line_file_start
        );
    }

    #[test]
    fn test_mmap_text_iter_at_eof() {
        // line_file_start 等于文件大小时，迭代器应立即返回空
        let mut mmap_text =
            MmapText::from_file_path("/home/unvdb/a.txt").expect("打开 /home/unvdb/a.txt 失败");
        let file_size = mmap_text.get_file_size();

        let line_state = LineState::builder()
            .line_index(0)
            .line_file_start(file_size)
            .build();

        let lines: Vec<LineStr> = mmap_text.iter(&line_state).collect();
        assert!(
            lines.is_empty(),
            "EOF 处迭代应返回空，实际返回 {} 行",
            lines.len()
        );
    }

    #[test]
    fn test_mmap_text_iter_single_line() {
        // 用临时文件验证单行行为
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("chappie_test_single_line.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"hello world\n").unwrap();
        }

        let mut mmap_text = MmapText::from_file_path(&path).unwrap();
        let line_state = LineState::builder()
            .line_index(0)
            .line_file_start(0)
            .build();

        let lines: Vec<LineStr> = mmap_text.iter(&line_state).collect();
        assert_eq!(lines.len(), 1, "单行文件应产出恰好一行");
        assert_eq!(extract_bytes(&lines[0]), b"hello world");
        assert_eq!(lines[0].line_file_start, 0);
        assert_eq!(lines[0].line_file_end, 11);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_mmap_text_iter_empty_lines() {
        // 验证空行（连续 \n）能正确迭代
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("chappie_test_empty_lines.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"aaa\n\nbbb\n\n").unwrap();
        }

        let mut mmap_text = MmapText::from_file_path(&path).unwrap();
        let line_state = LineState::builder()
            .line_index(0)
            .line_file_start(0)
            .build();

        let lines: Vec<LineStr> = mmap_text.iter(&line_state).collect();
        // "aaa\n\nbbb\n\n" → split gives ["aaa", "", "bbb", "", ""]
        // 迭代器去掉末尾空行后应为 ["aaa", "", "bbb", ""]
        assert_eq!(lines.len(), 4, "期望 4 行，实际 {}", lines.len());
        assert_eq!(extract_bytes(&lines[0]), b"aaa");
        assert_eq!(extract_bytes(&lines[1]), b"");
        assert_eq!(extract_bytes(&lines[2]), b"bbb");
        assert_eq!(extract_bytes(&lines[3]), b"");

        // 验证空行的 line_file_start / line_file_end 一致性
        assert_eq!(lines[1].line_file_start, 4); // 紧跟 "aaa\n" 之后
        assert_eq!(lines[1].line_file_end, 4); // 空行，start==end

        let _ = std::fs::remove_file(&path);
    }
}
