use crate::common::error::ChapError;
use crate::common::gap_buffer::GapBuffer;
use crate::textwarp::block::Block;
use crate::textwarp::ChapResult;
use crate::textwarp::EditText;
use crate::textwarp::Line;
use crate::textwarp::LineData;
use crate::textwarp::LineState;
use crate::textwarp::LineStr;
use crate::textwarp::RingVec;
use crate::textwarp::Text;
use crate::textwarp::TextIndex;
use crate::textwarp::TextSelect;
use crate::textwarp::CHUNK_NUM;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::io::Seek;
use std::path::Path;
//const PAGE_GROUP: usize = 1;
const HEX_CHUNK_SIZE: usize = 4 * 1024; // 每个块的大小
const HEX_GAP_SIZE: usize = 5;
pub(crate) const HEX_WITH: usize = 16; //16进制文本的宽度

// #[derive(Clone)]
// pub(crate) struct Block {
//     buffer: GapBuffer,
//     file_start: usize,
//     file_end: usize,
//     is_modified: bool,
// }

// impl Block {
//     fn text(&self, range: impl RangeBounds<usize>) -> GapBytes<'_> {
//         let start = match range.start_bound() {
//             std::ops::Bound::Included(&start) => start,
//             std::ops::Bound::Excluded(&start) => start + 1,
//             std::ops::Bound::Unbounded => self.file_start,
//         };
//         let end = match range.end_bound() {
//             std::ops::Bound::Included(&end) => end, //包含
//             std::ops::Bound::Excluded(&end) => end, //排除
//             std::ops::Bound::Unbounded => self.file_end,
//         };
//         assert!(start <= end && start >= self.file_start && end >= self.file_start);
//         self.buffer
//             .text((start - self.file_start)..(end - self.file_start))
//     }

//     fn text_len(&self) -> usize {
//         self.buffer.text_len()
//     }
// }

pub(crate) struct HexText {
    blocks: RingVec<Block>,       // 每个块4KB大小
    chk_iter: Block,              // 当前迭代的块 这个后续可以优化
    file: File,                   // 文件句柄
    cache: HashMap<usize, Block>, // 缓存已修改的块
    file_size: usize,             // 文件大小
    height: usize,                // 显示高度
}

impl HexText {
    pub(crate) fn from_file_path<P: AsRef<Path>>(
        filename: P,
        height: usize,
    ) -> ChapResult<HexText> {
        let mut file = File::open(filename)?;
        let file_size = file.metadata()?.len() as usize;
        let mut blocks = RingVec::with_capacity(CHUNK_NUM);

        let mut buf = [0u8; HEX_CHUNK_SIZE];
        let mut bytes_start = 0;
        for _ in 0..CHUNK_NUM {
            let mut buffer = GapBuffer::new(HEX_CHUNK_SIZE + HEX_GAP_SIZE);
            let mut bytes_read = 0;
            while bytes_read < HEX_CHUNK_SIZE {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    //跳出 for 循环
                    break;
                }
                bytes_read += n;
                buffer.insert(buffer.text_len(), &buf[..n]);
            }
            if bytes_read == 0 {
                break;
            }
            blocks.push(Block {
                data: buffer,
                source_file_start: bytes_start,
                source_file_end: bytes_start + bytes_read,
                block_id: blocks.len(),
                is_modified: false,
            });
            bytes_start += bytes_read;
        }
        let chk_iter = blocks
            .get(0)
            .unwrap_or(&Block {
                data: GapBuffer::new(0),
                source_file_start: 0,
                source_file_end: 0,
                block_id: 0,
                is_modified: false,
            })
            .clone();
        Ok(HexText {
            blocks: blocks,
            chk_iter: chk_iter,
            file,
            cache: HashMap::new(),
            file_size,
            height: height, // 初始高度为0，可以根据需要设置
        })
    }

    pub(crate) fn get_file_size(&self) -> usize {
        self.file_size
    }

    pub(crate) fn reset_chunks(&mut self, line_file_start: usize) {
        let n = (self.file_size + HEX_CHUNK_SIZE - 1) / HEX_CHUNK_SIZE;
        let last_chunk_address = (n - CHUNK_NUM) * HEX_CHUNK_SIZE;
        //如果没有找到块 从新重读chunks
        //通过line_file_start 计算在哪一个块 每个块的大小是 HEX_CHUNK_SIZE
        let chunk_start =
            (line_file_start / HEX_CHUNK_SIZE * HEX_CHUNK_SIZE).min(last_chunk_address);
        let block_num = chunk_start / HEX_CHUNK_SIZE;
        self.read_chunks(block_num, chunk_start).unwrap();
    }

    pub(crate) fn read_one_chunk(
        &mut self,
        block_num: usize,
        chunk_seek: usize,
    ) -> ChapResult<Block> {
        self.file
            .seek(std::io::SeekFrom::Start(chunk_seek as u64))?;
        let mut buf = [0u8; HEX_CHUNK_SIZE];
        let bytes_start = chunk_seek;
        let mut buffer = GapBuffer::new(HEX_CHUNK_SIZE + HEX_GAP_SIZE);
        let mut bytes_read = 0;
        while bytes_read < HEX_CHUNK_SIZE {
            let n = self.file.read(&mut buf)?;
            if n == 0 {
                //跳出 for 循环
                break;
            }
            bytes_read += n;
            buffer.insert(buffer.text_len(), &buf[..n]);
        }
        if bytes_read == 0 {
            return Err(ChapError::Unexpected("No data read from file".to_string()).into());
        }
        return Ok(Block {
            data: buffer,
            source_file_start: bytes_start,
            source_file_end: bytes_start + bytes_read,
            block_id: block_num,
            is_modified: false,
        });
    }

    pub(crate) fn read_chunks(&mut self, block_num: usize, chunk_seek: usize) -> ChapResult<()> {
        self.file
            .seek(std::io::SeekFrom::Start(chunk_seek as u64))?;
        let mut blocks = RingVec::with_capacity(CHUNK_NUM);

        let mut buf = [0u8; HEX_CHUNK_SIZE];
        let mut bytes_start = chunk_seek;
        for i in 0..CHUNK_NUM {
            let mut buffer = GapBuffer::new(HEX_CHUNK_SIZE + HEX_GAP_SIZE);
            let mut bytes_read = 0;
            while bytes_read < HEX_CHUNK_SIZE {
                let n = self.file.read(&mut buf)?;
                if n == 0 {
                    //跳出 for 循环
                    break;
                }
                bytes_read += n;
                buffer.insert(buffer.text_len(), &buf[..n]);
            }
            if bytes_read == 0 {
                break;
            }
            blocks.push(Block {
                data: buffer,
                source_file_start: bytes_start,
                source_file_end: bytes_start + bytes_read,
                block_id: block_num + i,
                is_modified: false,
            });
            bytes_start += bytes_read;
        }
        self.blocks = blocks;
        return Ok(());
    }

    pub(crate) fn read_last_chunk(&mut self, block_num: usize, file_seek: usize) -> ChapResult<()> {
        if file_seek >= self.file_size {
            return Ok(());
        }
        self.file.seek(std::io::SeekFrom::Start(file_seek as u64))?;
        let mut buffer = GapBuffer::new(HEX_CHUNK_SIZE + HEX_GAP_SIZE);
        let mut bytes_read = 0;
        let mut buf = [0u8; 1024];
        while bytes_read < HEX_CHUNK_SIZE {
            let n = self.file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            bytes_read += n;
            buffer.insert(buffer.text_len(), &buf[..n]);
        }

        //弹出最后一个块
        let block2 = self.blocks.remove_last();
        if let Some(c) = block2 {
            if c.is_modified {
                self.cache.insert(c.source_file_start, c);
            }
        }
        self.blocks.push_front(Block {
            data: buffer,
            source_file_start: file_seek,
            source_file_end: file_seek + bytes_read,
            block_id: block_num,
            is_modified: false,
        });
        Ok(())
    }

    pub(crate) fn read_next_chunk(&mut self, block_num: usize, file_seek: usize) -> ChapResult<()> {
        if file_seek >= self.file_size {
            return Ok(());
        }
        self.file.seek(std::io::SeekFrom::Start(file_seek as u64))?;
        let mut buffer = GapBuffer::new(HEX_CHUNK_SIZE + HEX_GAP_SIZE);
        let mut bytes_read = 0;
        let mut buf = [0u8; 1024];
        while bytes_read < HEX_CHUNK_SIZE {
            let n = self.file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            bytes_read += n;
            buffer.insert(buffer.text_len(), &buf[..n]);
        }

        //弹出第一个块
        let block0 = self.blocks.remove(0);
        if let Some(c) = block0 {
            if c.is_modified {
                self.cache.insert(c.source_file_start, c);
            }
        }
        self.blocks.push(Block {
            data: buffer,
            source_file_start: file_seek,
            source_file_end: file_seek + bytes_read,
            block_id: block_num,
            is_modified: false,
        });
        Ok(())
    }

    fn block_pos(&self, block_id: usize) -> Option<usize> {
        self.blocks.iter().position(|b| b.block_id == block_id)
    }

    fn sync_loaded_block_metadata_from(&mut self, start_idx: usize) {
        if self.blocks.is_empty() || start_idx >= self.blocks.len() {
            return;
        }
        let mut next_start = if start_idx == 0 {
            self.blocks.get(0).map_or(0, |b| b.source_file_start)
        } else {
            self.blocks
                .get(start_idx - 1)
                .map_or(0, |b| b.source_file_end)
        };
        for i in start_idx..self.blocks.len() {
            if let Some(block) = self.block_mut_at_pos(i) {
                block.source_file_start = next_start;
                block.source_file_end = next_start + block.block_size();
                next_start = block.source_file_end;
            }
        }
    }

    fn block_mut_at_pos(&mut self, pos: usize) -> Option<&mut Block> {
        self.blocks.iter_mut().nth(pos)
    }
}

impl TextIndex for HexText {
    fn get_page_offset(&self, line_num: usize) -> LineState {
        let start_page_num = line_num / self.height;
        let start_line_num = (start_page_num * self.height).saturating_sub(1);
        let line_file_start = start_line_num * HEX_WITH;
        let state = LineState::builder()
            .line_index(0)
            .line_offset(0)
            .block_num(0)
            .block_line_index(0)
            .block_offset(0)
            .line_file_start(line_file_start)
            .start_line_num(start_line_num)
            .start_page_num(start_page_num)
            .build();
        state
    }

    fn set_page_offset(&mut self, page_num: usize, page_offset: LineState) {
        // 这里可以实现设置页偏移的逻辑
        // 目前没有具体实现
    }
}

impl EditText for HexText {
    fn backspace(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        mut count: usize,
        line_meta: &LineState,
    ) -> ChapResult<Vec<u8>> {
        let _ = cursor_y;
        if count == 0 {
            return Ok(vec![]);
        }
        let cursor_pos = line_meta.get_line_file_start() + bytes_cursor;
        let delete_start = cursor_pos.saturating_sub(count);
        let delete_end = cursor_pos;
        let mut deleted = Vec::with_capacity(count);
        let mut first_touched = None;

        for pos in (0..self.blocks.len()).rev() {
            let (block_start, block_end) = {
                let block = self.blocks.get(pos).unwrap();
                (block.source_file_start, block.source_file_end)
            };
            let overlap_start = delete_start.max(block_start);
            let overlap_end = delete_end.min(block_end);
            if overlap_start >= overlap_end {
                continue;
            }
            let local_end = overlap_end - block_start;
            let local_count = overlap_end - overlap_start;
            if local_count == 0 {
                break;
            }
            let block = self.block_mut_at_pos(pos).unwrap();
            let chunk = block.backspace(local_end, local_count);
            deleted.splice(0..0, chunk);
            count = count.saturating_sub(local_count);
            first_touched = Some(pos);
        }

        let deleted_len = deleted.len();
        if deleted_len > 0 {
            self.file_size = self.file_size.saturating_sub(deleted_len);
            if let Some(pos) = first_touched {
                self.sync_loaded_block_metadata_from(pos);
            }
        }

        Ok(deleted)
    }

    fn insert_bytes(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        c: &[u8],
        is_overwrite: bool,
    ) -> ChapResult<()> {
        let _ = cursor_y;
        if c.is_empty() {
            return Ok(());
        }
        if is_overwrite {
            //覆盖模式 先删除后插入
            self.backspace(cursor_y, bytes_cursor + c.len(), c.len(), line_meta)?;
        }
        let insert_pos = line_meta.get_line_file_start() + bytes_cursor;
        let mut target_pos = None;
        let mut local_offset = 0;
        for (pos, block) in self.blocks.iter().enumerate() {
            if insert_pos >= block.source_file_start && insert_pos <= block.source_file_end {
                target_pos = Some(pos);
                local_offset = insert_pos - block.source_file_start;
                break;
            }
        }
        if target_pos.is_none() && !self.blocks.is_empty() && insert_pos == self.get_file_size() {
            let pos = self.blocks.len() - 1;
            target_pos = Some(pos);
            local_offset = self.blocks.get(pos).unwrap().block_size();
        }
        if let Some(pos) = target_pos {
            let block = self.block_mut_at_pos(pos).unwrap();
            block.insert(local_offset, c);
            self.file_size += c.len();
            self.sync_loaded_block_metadata_from(pos);
        } else {
            return Err(
                ChapError::Unexpected("insert position outside loaded blocks".to_string()).into(),
            );
        }

        Ok(())
    }

    fn insert_char(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        c: char,
    ) -> ChapResult<()> {
        //不需要实现
        Ok(())
    }

    fn insert_newline(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
    ) -> ChapResult<()> {
        //不需要实现
        Ok(())
    }

    fn delete_line(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
    ) -> ChapResult<()> {
        //不需要实现
        Ok(())
    }

    fn make_backup<P: AsRef<Path>>(&mut self, backup_name: P) -> ChapResult<()> {
        Ok(())
    }

    fn rollback() -> ChapResult<()> {
        Ok(())
    }

    fn ensure_block_loaded(&mut self, _block_num: usize) -> ChapResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::textwarp::{EditText, Line, Text, TextSelect};
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_temp_file(bytes: &[u8]) -> NamedTempFile {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        tmp.write_all(bytes).expect("write temp bytes");
        tmp.flush().expect("flush temp file");
        tmp
    }

    fn make_hex_text(bytes: &[u8]) -> HexText {
        let tmp = write_temp_file(bytes);
        HexText::from_file_path(tmp.path(), 16).expect("open HexText")
    }

    fn line_state_for_cursor(text: &HexText, cursor_pos: usize) -> LineState {
        let line_file_start = (cursor_pos / HEX_WITH) * HEX_WITH;
        let line_file_end = (line_file_start + HEX_WITH).min(text.get_file_size());
        for block in text.blocks.iter() {
            if line_file_start >= block.source_file_start && line_file_start < block.source_file_end
            {
                return LineState::builder()
                    .block_num(block.block_id)
                    .block_offset(line_file_start - block.source_file_start)
                    .line_file_start(line_file_start)
                    .line_file_end(line_file_end)
                    .line_offset(0)
                    .txt_len(line_file_end.saturating_sub(line_file_start))
                    .char_len(line_file_end.saturating_sub(line_file_start))
                    .line_index(line_file_start / HEX_WITH)
                    .line_num(line_file_start / HEX_WITH + 1)
                    .build();
            }
        }
        panic!("cursor_pos {cursor_pos} not covered by any loaded block");
    }

    fn collect_all_bytes(text: &mut HexText) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(text.get_file_size());
        for block in text.blocks.iter_mut() {
            bytes.extend_from_slice(block.as_continuous());
        }
        bytes
    }

    fn assert_loaded_blocks_contiguous(text: &HexText) {
        let mut expected = text.blocks.get(0).map_or(0, |b| b.source_file_start);
        for block in text.blocks.iter() {
            assert_eq!(block.source_file_start, expected);
            assert_eq!(
                block.source_file_end,
                block.source_file_start + block.block_size()
            );
            expected = block.source_file_end;
        }
        assert_eq!(
            text.get_file_size(),
            text.blocks.iter().map(|b| b.block_size()).sum()
        );
    }

    fn line_to_bytes(line: LineStr<'_>) -> Vec<u8> {
        match line.get_data() {
            LineData::Own(v) => v,
            LineData::Bytes(v) => v.to_vec(),
            LineData::GapBytes(v) => v.to_vec(),
            LineData::GapBlockBytes(v1, v2) => {
                let mut bytes = v1.to_vec();
                bytes.extend_from_slice(&v2.to_vec());
                bytes
            }
        }
    }

    #[test]
    fn test_hex_get_line_across_chunk_boundary_returns_full_16_bytes() {
        let data: Vec<u8> = (0..(HEX_CHUNK_SIZE + 64))
            .map(|i| (i % 251) as u8)
            .collect();
        let text = make_hex_text(&data);
        let line_start = HEX_CHUNK_SIZE - 8;
        let state = LineState::builder()
            .line_file_start(line_start)
            .line_file_end(line_start + HEX_WITH)
            .build();

        let line = text.get_line(&state).expect("line exists");
        assert_eq!(line.text_len(), HEX_WITH);
        assert_eq!(
            line_to_bytes(line),
            data[line_start..line_start + HEX_WITH].to_vec()
        );
    }

    #[test]
    fn test_hex_iter_u8_reads_large_file_across_chunks() {
        let data: Vec<u8> = (0..(HEX_CHUNK_SIZE * 3 + 257))
            .map(|i| ((i * 17 + 3) % 256) as u8)
            .collect();
        let mut text = make_hex_text(&data);

        let roundtrip: Vec<u8> = text.iter_u8(0, 0, 0).collect();
        assert_eq!(roundtrip, data);
    }

    #[test]
    fn test_hex_text_from_sel_spans_multiple_chunks() {
        let data: Vec<u8> = (0..(HEX_CHUNK_SIZE * 2 + 97))
            .map(|i| ((i * 9 + 11) % 256) as u8)
            .collect();
        let text = make_hex_text(&data);
        let start = HEX_CHUNK_SIZE - 11;
        let end = HEX_CHUNK_SIZE + 23;

        let selected = text.text_from_sel(&TextSelect::from_select(start, end));
        assert_eq!(selected, data[start..=end]);
    }

    #[test]
    fn test_hex_insert_bytes_updates_stream_and_loaded_metadata_at_chunk_boundary() {
        let data: Vec<u8> = (0..(HEX_CHUNK_SIZE * 2 + 32))
            .map(|i| (i % 256) as u8)
            .collect();
        let mut expected = data.clone();
        let mut text = make_hex_text(&data);
        let insert_pos = HEX_CHUNK_SIZE - 3;
        let payload = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE];
        let state = line_state_for_cursor(&text, insert_pos);

        text.insert_bytes(0, insert_pos % HEX_WITH, &state, &payload, false)
            .expect("insert across chunk boundary");
        expected.splice(insert_pos..insert_pos, payload);

        assert_eq!(collect_all_bytes(&mut text), expected);
        assert_loaded_blocks_contiguous(&text);
    }

    #[test]
    fn test_hex_backspace_across_chunk_boundary_returns_deleted_bytes_and_updates_stream() {
        let data: Vec<u8> = (0..(HEX_CHUNK_SIZE * 2 + 64))
            .map(|i| ((i * 5 + 7) % 256) as u8)
            .collect();
        let mut expected = data.clone();
        let mut text = make_hex_text(&data);
        let cursor_pos = HEX_CHUNK_SIZE + 5;
        let delete_count = 19;
        let state = line_state_for_cursor(&text, cursor_pos);

        let deleted = text
            .backspace(0, cursor_pos % HEX_WITH, delete_count, &state)
            .expect("backspace across boundary");
        let start = cursor_pos - delete_count;
        let expected_deleted = expected[start..cursor_pos].to_vec();
        expected.drain(start..cursor_pos);

        assert_eq!(deleted, expected_deleted);
        assert_eq!(collect_all_bytes(&mut text), expected);
        assert_loaded_blocks_contiguous(&text);
    }

    #[test]
    fn test_hex_large_500_mixed_insert_bytes_and_backspace_match_reference_model() {
        let data: Vec<u8> = (0..(HEX_CHUNK_SIZE * 4 + 173))
            .map(|i| ((i * 31 + 19) % 256) as u8)
            .collect();
        let mut expected = data.clone();
        let mut text = make_hex_text(&data);

        for step in 0..500usize {
            if step % 3 == 0 || expected.len() < 8 {
                let insert_pos = (step * 97 + 13) % (expected.len() + 1);
                let payload_len = (step % 11) + 1;
                let payload: Vec<u8> = (0..payload_len)
                    .map(|i| ((step * 7 + i * 29 + 5) % 256) as u8)
                    .collect();
                let state = line_state_for_cursor(&text, insert_pos.min(text.get_file_size()));
                text.insert_bytes(0, insert_pos % HEX_WITH, &state, &payload, false)
                    .expect("mixed insert");
                expected.splice(insert_pos..insert_pos, payload);
            } else {
                let delete_count = ((step * 5) % 7) + 1;
                let cursor_pos = ((step * 53 + 29) % (expected.len() - 1)) + 1;
                let actual_delete = delete_count.min(cursor_pos);
                let state = line_state_for_cursor(&text, cursor_pos);
                let deleted = text
                    .backspace(0, cursor_pos % HEX_WITH, actual_delete, &state)
                    .expect("mixed delete");
                let start = cursor_pos - actual_delete;
                let expected_deleted = expected[start..cursor_pos].to_vec();
                expected.drain(start..cursor_pos);
                assert_eq!(deleted, expected_deleted, "step {step}");
            }

            assert_eq!(
                collect_all_bytes(&mut text),
                expected,
                "byte stream mismatch at step {step}"
            );
            assert_loaded_blocks_contiguous(&text);
        }
    }
}

impl Text for HexText {
    type LineItem<'a> = LineStr<'a>;
    fn get_file_size(&self) -> usize {
        self.file_size
    }

    fn get_next_line_state(&self, state: &LineState) -> Option<LineState> {
        let mut line_index = state.get_line_index();
        let mut line_end = state.get_line_end();
        let mut line_file_start = state.get_line_file_start();

        let line = self.get_line(&state).unwrap();
        if state.get_line_end() == line.text_len() {
            line_file_start = state.get_line_file_end();
            line_end = 0;
            line_index += 1;
        }

        let p = LineState::builder()
            .line_index(line_index)
            .line_offset(line_end)
            .line_file_start(line_file_start)
            //.start_line_num(state.get_line_num())
            // .start_page_num(state.get_line_num() / self.height)
            .build();
        Some(p)
    }

    fn has_pre_line(&self, meta: &LineState) -> bool {
        meta.get_line_file_start() > 0 || meta.get_line_offset() > 0
    }

    fn get_pre_line_state(&mut self, state: &LineState) -> Option<LineState> {
        if state.get_line_file_start() == 0 && state.get_line_offset() == 0 {
            return None;
        }

        let prev_start = state.get_line_file_start().saturating_sub(HEX_WITH);
        let prev_end = (prev_start + HEX_WITH).min(self.file_size);
        let txt_len = prev_end.saturating_sub(prev_start);

        let (block_num, block_offset) = self
            .blocks
            .iter()
            .find(|b| prev_start >= b.source_file_start && prev_start < b.source_file_end)
            .map(|b| (b.block_id, prev_start - b.source_file_start))
            .unwrap_or((prev_start / HEX_CHUNK_SIZE, prev_start % HEX_CHUNK_SIZE));

        Some(
            LineState::builder()
                .line_index(prev_start / HEX_WITH)
                .line_offset(0)
                .line_file_start(prev_start)
                .line_file_end(prev_end)
                .txt_len(txt_len)
                .char_len(txt_len)
                .block_num(block_num)
                .block_offset(block_offset)
                .build(),
        )
    }

    fn text_from_sel(&self, sel: &TextSelect) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut start = sel.get_start();
        let end = sel.get_end();
        for s in self.blocks.iter() {
            // 跳过选区起点位于此块之后的情况
            if start >= s.source_file_end {
                continue;
            }
            // 如果选区在此块之前结束，则无需继续
            if end < s.source_file_start {
                break;
            }
            // 计算当前块与选区的重叠范围
            let from = (start.max(s.source_file_start) - s.source_file_start) as usize;
            let to = (end.min(s.source_file_end) - s.source_file_start) as usize;
            // 提取并追加子片段
            buf.extend_from_slice(&s.data.text(from..=to).to_vec());
            // 如果选区在此块内完全结束，则跳出循环
            if end <= s.source_file_end {
                break;
            }
            // 更新起点为当前块末尾，继续下一块
            start = s.source_file_end;
        }
        buf
    }

    fn get_line<'a>(&'a self, state: &LineState) -> Option<LineStr<'a>> {
        let with = state.line_file_end - state.line_file_start;
        for (i, b) in self.blocks.iter().enumerate() {
            if state.line_file_start > b.source_file_end
                || state.line_file_start < b.source_file_start
            {
                continue;
            }
            let buffer = b.text_from_file_seek(state.line_file_start..);
            let line_start = state.line_file_start;
            // 如果不足以填充 with 宽度 说明是跨块了
            if with > buffer.len() {
                let len = buffer.len();
                // 一行跨两个块数据
                if i < self.blocks.len() - 1 {
                    if let Some(c1) = self.blocks.get(i + 1) {
                        let mut v: Vec<u8> = Vec::with_capacity(with);
                        v.extend_from_slice(buffer.left());
                        v.extend_from_slice(buffer.right());
                        let remaining = with - buffer.len();
                        let buf1 = c1.text_from_file_seek(state.line_file_start + len..);
                        if remaining >= buf1.len() {
                            v.extend_from_slice(buf1.left());
                            v.extend_from_slice(buf1.right());
                            return Some(LineStr {
                                data: LineData::Own(v),
                                block_id: b.block_id,
                                block_offset: state.line_file_start - b.source_file_start,
                                line_file_start: line_start,
                                line_file_end: line_start + len + buf1.len(),
                            });
                        } else {
                            let buf2 = buf1.text(..remaining);
                            v.extend_from_slice(buf2.left());
                            v.extend_from_slice(buf2.right());
                            return Some(LineStr {
                                data: LineData::Own(v),
                                block_id: b.block_id,
                                block_offset: state.line_file_start - b.source_file_start,
                                line_file_start: line_start,
                                line_file_end: line_start + with,
                            });
                        }
                    } else {
                        return Some(LineStr {
                            data: LineData::GapBytes(buffer),
                            block_id: b.block_id,
                            block_offset: state.line_file_start - b.source_file_start,
                            line_file_start: line_start,
                            line_file_end: state.line_file_end,
                        });
                    }
                } else {
                    return Some(LineStr {
                        data: LineData::GapBytes(buffer),
                        block_id: b.block_id,
                        block_offset: state.line_file_start - b.source_file_start,
                        line_file_start: line_start,
                        line_file_end: line_start + len,
                    });
                }
            } else {
                return Some(LineStr {
                    data: LineData::GapBytes(buffer.text(..with)),
                    block_id: b.block_id,
                    block_offset: state.line_file_start - b.source_file_start,
                    line_file_start: line_start,
                    line_file_end: line_start + with,
                });
            }
        }
        return Some(LineStr {
            // line: buffer,
            data: LineData::Bytes(&[]),
            block_id: 0,
            block_offset: 0,
            line_file_start: 0,
            line_file_end: 0,
        });
    }

    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize {
        line_end - line_start
    }

    fn has_next_line(&self, meta: &LineState) -> bool {
        if meta.line_file_end >= self.file_size {
            return false;
        }
        return true;
    }

    fn iter_rev<'a>(&'a mut self, line_state: &LineState) -> impl Iterator<Item = LineStr<'a>> {
        self.iter(line_state)
    }

    fn iter<'a>(&'a mut self, line_state: &LineState) -> impl Iterator<Item = LineStr<'a>> {
        let mut j = None;
        loop {
            if line_state.line_file_start >= self.file_size {
                return HexTextIter::new([None, None], HEX_WITH, line_state.line_file_start);
            }
            for (i, block) in self.blocks.iter().enumerate() {
                if line_state.line_file_start >= block.source_file_start
                    && line_state.line_file_start < block.source_file_end
                {
                    j = Some(i);
                    break;
                }
            }
            if let Some(j) = j {
                if j == 0 {
                    let b = self.blocks.get(0).unwrap();
                    let block_seek = b.source_file_start;
                    let block_num = b.block_id;
                    if block_seek == 0 {
                        //已经是第一个块无需弹出
                        return HexTextIter::new(
                            [self.blocks.get(0), self.blocks.get(1)],
                            HEX_WITH,
                            line_state.line_file_start,
                        );
                    } else {
                        //读取上一个块 把最后一个块弹出
                        let last_block_seek = block_seek.saturating_sub(HEX_CHUNK_SIZE);
                        self.read_last_chunk(block_num - 1, last_block_seek)
                            .unwrap();
                        // for c in self.blocks.iter() {}
                        return HexTextIter::new(
                            [self.blocks.get(1), self.blocks.get(2)],
                            HEX_WITH,
                            line_state.line_file_start,
                        );
                    }
                } else if j == self.blocks.len() - 1 {
                    //最后一个块
                    //读取下一个块 把第一个块弹出
                    let next_file_seek = self.blocks.get(j).unwrap().source_file_end;
                    let block_num = self.blocks.get(j).unwrap().block_id;
                    if next_file_seek >= self.file_size {
                        return HexTextIter::new(
                            [self.blocks.get(j), None],
                            HEX_WITH,
                            line_state.line_file_start,
                        );
                    } else {
                        self.read_next_chunk(block_num + 1, next_file_seek).unwrap();
                        return HexTextIter::new(
                            [self.blocks.get(j - 1), self.blocks.get(j)],
                            HEX_WITH,
                            line_state.line_file_start,
                        );
                    }
                } else {
                    //不是最后一个块
                    return HexTextIter::new(
                        [self.blocks.get(j), self.blocks.get(j + 1)],
                        HEX_WITH,
                        line_state.line_file_start,
                    );
                }
            }
            self.reset_chunks(line_state.line_file_start);
        }
    }

    fn iter_u8<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_file_start: usize,
    ) -> impl Iterator<Item = u8> {
        let mut j = None;
        for (i, block) in self.blocks.iter().enumerate() {
            if line_file_start >= block.source_file_start && line_file_start < block.source_file_end
            {
                j = Some(i);
                break;
            }
        }
        if let Some(j) = j {
            self.chk_iter = self.blocks.get(j).unwrap().clone();
            return HexTextU8Iter::new(
                self,
                line_file_start - self.blocks.get(j).unwrap().source_file_start,
            );
        }
        // 如果没有找到块 从新重读chunks
        // 通过line_file_start 计算在哪一个块 每个块的大小是 HEX_CHUNK_SIZE
        let chunk_start = line_file_start / HEX_CHUNK_SIZE * HEX_CHUNK_SIZE;
        let block_num = chunk_start / HEX_CHUNK_SIZE;
        let chk_iter = self.read_one_chunk(block_num, chunk_start).unwrap();
        self.chk_iter = chk_iter;
        return HexTextU8Iter::new(self, line_file_start - self.chk_iter.source_file_start);
    }

    fn search(&mut self, partten: &[u8], state: &LineState) -> ChapResult<Option<Vec<LineState>>> {
        Ok(None)
    }
}

struct HexTextU8Iter<'a> {
    hex_text: &'a mut HexText,
    i: usize,
}

impl<'a> HexTextU8Iter<'a> {
    fn new(hex_text: &'a mut HexText, i: usize) -> HexTextU8Iter<'a> {
        HexTextU8Iter { hex_text, i: i }
    }
}

impl<'a> Iterator for HexTextU8Iter<'a> {
    type Item = u8;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // 获取当前 chunk 并从中读取一字节
            let chunk = &self.hex_text.chk_iter;
            let buffer = chunk.data.text(..);
            let (a, b) = buffer.as_slice();
            let total = a.len() + b.len();

            if self.i < total {
                let byte = if self.i < a.len() {
                    a[self.i]
                } else {
                    b[self.i - a.len()]
                };
                self.i += 1;
                return Some(byte);
            }
            let next_block_seek = self.hex_text.chk_iter.source_file_end;
            let next_block_num = self.hex_text.chk_iter.block_id + 1;
            if let Ok(chk) = self
                .hex_text
                .read_one_chunk(next_block_num, next_block_seek)
            {
                self.hex_text.chk_iter = chk;
            } else {
                return None; // 如果没有更多数据，返回 None
            }
            self.i = 0;
        }
    }
}

struct HexTextEmptyIter;

impl Iterator for HexTextEmptyIter {
    type Item = LineStr<'static>;

    fn next(&mut self) -> Option<Self::Item> {
        None
    }
}

struct HexTextIter<'a> {
    hex_chunk: [Option<&'a Block>; 2],
    with: usize,
    line_file_start: usize,
}

impl<'a> HexTextIter<'a> {
    fn new(
        hex_chunk: [Option<&'a Block>; 2],
        with: usize,
        line_file_start: usize,
    ) -> HexTextIter<'a> {
        HexTextIter {
            hex_chunk,
            with,
            line_file_start: line_file_start,
        }
    }
}

impl<'a> Iterator for HexTextIter<'a> {
    type Item = LineStr<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        for (i, block) in self.hex_chunk.iter().enumerate() {
            if let Some(b) = block {
                if self.line_file_start >= b.source_file_end
                    || self.line_file_start < b.source_file_start
                {
                    continue;
                }
                let line_start = self.line_file_start;
                let buffer = b.text_from_file_seek(line_start..);
                // 长度大于 buffer 说明当前chunk 不足以显示一行
                if self.with > buffer.len() {
                    let len = buffer.len();
                    if i == 0 {
                        // 从第一个块读取完毕
                        if let Some(b1) = self.hex_chunk[1] {
                            let mut v: Vec<u8> = Vec::with_capacity(self.with);
                            v.extend_from_slice(buffer.left());
                            v.extend_from_slice(buffer.right());
                            let remaining = self.with - buffer.len();
                            //从下一个块读取
                            let buf1 = b1.text_from_file_seek(line_start + len..);
                            //继续读取
                            if remaining >= buf1.len() {
                                // let buf2 = buf1.text(..need);
                                v.extend_from_slice(buf1.left());
                                v.extend_from_slice(buf1.right());
                                self.line_file_start += len + buf1.len();
                                return Some(LineStr {
                                    data: LineData::Own(v),
                                    block_id: b.block_id,
                                    block_offset: line_start - b.source_file_start,
                                    line_file_start: line_start,
                                    line_file_end: line_start + len + buf1.len(),
                                });
                            } else {
                                self.line_file_start += self.with;
                                let buf2 = buf1.text(..remaining);
                                v.extend_from_slice(buf2.left());
                                v.extend_from_slice(buf2.right());
                                return Some(LineStr {
                                    data: LineData::Own(v),
                                    block_id: b.block_id,
                                    block_offset: line_start - b.source_file_start,
                                    line_file_start: line_start,
                                    line_file_end: line_start + self.with,
                                });
                            }
                        } else {
                            self.line_file_start += len;
                            return Some(LineStr {
                                data: LineData::GapBytes(buffer),
                                block_id: b.block_id,
                                block_offset: line_start - b.source_file_start,
                                line_file_start: line_start,
                                line_file_end: line_start + len,
                            });
                        }
                    } else {
                        self.line_file_start += len;
                        return Some(LineStr {
                            data: LineData::GapBytes(buffer),
                            block_id: b.block_id,
                            block_offset: line_start - b.source_file_start,
                            line_file_start: line_start,
                            line_file_end: line_start + len,
                        });
                    }
                } else {
                    self.line_file_start += self.with;
                    return Some(LineStr {
                        // line: buffer.text(..self.with),
                        data: LineData::GapBytes(buffer.text(..self.with)),
                        block_id: b.block_id,
                        block_offset: line_start - b.source_file_start,
                        line_file_start: line_start,
                        line_file_end: line_start + self.with,
                    });
                }
            }
        }
        return None;
    }
}
