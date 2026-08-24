use crate::searcher::memchr::memchr;
use crate::textwarp::ChapResult;
use crate::textwarp::GapBuffer;
use crate::textwarp::GapBytes;
use crate::ChapError;
use crc::Crc;
use crc::CRC_32_ISO_HDLC;
use std::io;
use std::io::Seek;
use std::io::SeekFrom;

pub(crate) const CHAR_GAP_SIZE: usize = 64;
pub(crate) const BLOCK_SIZE: usize = 4096;
pub(crate) const BLOKK_NUM: usize = 8;
pub(crate) const MAX_LINE_SIZE: usize = 4096; //最大行长度4KB

pub(crate) type BlockId = usize;
pub(crate) const EMPTY_BLOCK_ID: BlockId = usize::MAX;

#[derive(Clone)]
pub(crate) struct LineSpan {
    pub(crate) index_num: usize,
    pub(crate) block_start: usize,
    pub(crate) block_end: usize,
    pub(crate) is_complete: bool,
}

#[derive(Clone)]
pub(crate) struct BlockIndex {
    pub(crate) logic_file_start: usize,  //块在文件逻辑开始位置
    pub(crate) source_file_start: usize, //块在原始 backing file 中的位置
    pub(crate) source_file_end: usize,   //原始块在文件结束位置 不能改变
    pub(crate) block_id: BlockId,        //块的稳定标识（分裂后不变）
    pub(crate) logic_block_size: usize,  //逻辑块的大小
    pub(crate) check_sum: u32,           //保存的时候会更新 sum
}

impl BlockIndex {
    pub(crate) fn file_end(&self) -> usize {
        self.logic_file_start + self.logic_block_size
    }

    pub(crate) fn source_file_end(&self) -> usize {
        self.source_file_end
    }

    pub(crate) fn from_block_bytes(
        file_start: usize,
        source_file_start: usize,
        source_file_end: usize,
        block_id: BlockId,
        actual_len: usize,
        sum: u32,
    ) -> ChapResult<BlockIndex> {
        Ok(BlockIndex {
            logic_file_start: file_start, //块在文件开始位置
            source_file_start: source_file_start,
            source_file_end: source_file_end,
            block_id: block_id,
            logic_block_size: actual_len, //块的大小
            check_sum: sum,
        })
    }
}

pub(crate) enum BlockPtr<'a> {
    Own(Block),
    Borrowed(&'a Block),
}

impl<'a> BlockPtr<'a> {
    pub(crate) fn from_block(block: &'a Block) -> Self {
        BlockPtr::Borrowed(block)
    }

    pub(crate) fn from_block_owned(block: Block) -> Self {
        BlockPtr::Own(block)
    }

    pub(crate) fn as_block(&self) -> &Block {
        match self {
            BlockPtr::Own(block) => block,
            BlockPtr::Borrowed(block) => block,
        }
    }

    pub(crate) fn as_block_mut(&mut self) -> &mut Block {
        match self {
            BlockPtr::Own(block) => block,
            BlockPtr::Borrowed(_) => panic!("Cannot get mutable reference from borrowed block"),
        }
    }
}

#[derive(Clone)]
//按块加载文件 每个块4KB大小
pub(crate) struct Block {
    pub(crate) data: GapBuffer,          //每一个块使用 GapBuffer 存储
    pub(crate) source_file_start: usize, //原始块在文件开始位置 不能改变
    pub(crate) source_file_end: usize,   //原始块在文件结束位置 不能改变
    pub(crate) block_id: BlockId,        //块的稳定标识（分裂后不变）
    pub(crate) is_modified: bool,        //块是否被修改
}

impl Block {
    pub(crate) fn text_from_file_seek(
        &self,
        range: impl std::ops::RangeBounds<usize>,
    ) -> GapBytes<'_> {
        let start = match range.start_bound() {
            std::ops::Bound::Included(&start) => start,
            std::ops::Bound::Excluded(&start) => start + 1,
            std::ops::Bound::Unbounded => self.source_file_start,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(&end) => end, //包含
            std::ops::Bound::Excluded(&end) => end, //排除
            std::ops::Bound::Unbounded => self.source_file_end,
        };
        assert!(start <= end && start >= self.source_file_start && end >= self.source_file_start);
        self.data
            .text((start - self.source_file_start)..(end - self.source_file_start))
    }

    pub(crate) fn as_continuous(&mut self) -> &[u8] {
        self.data.as_continuous()
    }

    pub(crate) fn from_reader<T: io::Read + Seek>(
        reader: &mut T,
        buf: &mut [u8],
        file_start: usize,
        block_id: BlockId,
        check_sum: Option<u32>,
    ) -> ChapResult<(Self, Option<BlockIndex>)> {
        reader.seek(SeekFrom::Start(file_start as u64))?;
        let n = reader.read(buf)?;
        if n == 0 {
            return Err(ChapError::EOF);
        }

        // 调整到有效的UTF-8字符边界
        let actual_len = match str::from_utf8(&buf[..n]) {
            Ok(_) => n,                // 整个块有效，直接使用
            Err(e) => e.valid_up_to(), // 使用最后一个有效字符的边界
        };

        // 如果有剩余字节（字符被分割），回退文件读取位置
        if actual_len < n {
            let seek_back = n - actual_len;
            reader.seek(SeekFrom::Current(-(seek_back as i64)))?;
        }

        // 使用有效部分的数据
        let valid_data = &buf[..actual_len];
        let sum = crc_checksum(valid_data);
        if let Some(sum1) = check_sum {
            if sum != sum1 {
                panic!("Checksum Mismatch Exception")
            }
        }
        let block_index = if check_sum.is_none() {
            let block_index = BlockIndex::from_block_bytes(
                file_start,
                file_start,
                file_start + actual_len,
                block_id,
                actual_len,
                sum,
            )?;
            Some(block_index)
        } else {
            None
        };

        let block = Block {
            data: GapBuffer::from_bytes(valid_data, CHAR_GAP_SIZE),
            source_file_start: file_start,
            source_file_end: file_start + actual_len,
            block_id: block_id,
            is_modified: false,
        };

        Ok((block, block_index))
    }

    pub(crate) fn insert(&mut self, block_offset: usize, bytes: &[u8]) {
        self.data.insert(block_offset, bytes);
        self.is_modified = true;
    }

    pub(crate) fn backspace(&mut self, block_offset: usize, count: usize) -> Vec<u8> {
        let deleted_bytes = self
            .data
            .text(block_offset.saturating_sub(count)..block_offset)
            .to_vec();
        self.data.backspace(block_offset, count);
        self.is_modified = true;
        deleted_bytes
    }

    pub(crate) fn backspace_last(&mut self, count: usize) {
        let end = self.block_size();
        self.data.backspace(end, count.min(end));
        self.is_modified = true;
    }

    pub(crate) fn block_size(&self) -> usize {
        self.data.text_len()
    }

    fn scan_line_spans<F>(&self, mut f: F)
    where
        F: FnMut(LineSpan) -> bool,
    {
        let (left, right) = self.data.slices();
        let mut start = 0usize;
        let mut base = 0usize;
        let mut index_num = 0usize;

        for slice in [left, right] {
            let mut offset = 0usize;
            while offset < slice.len() {
                if let Some(found) = memchr(&slice[offset..], b'\n') {
                    let end = base + offset + found + 1;
                    if !f(LineSpan {
                        index_num,
                        block_start: start,
                        block_end: end,
                        is_complete: true,
                    }) {
                        return;
                    }
                    index_num += 1;
                    start = end;
                    offset += found + 1;
                } else {
                    break;
                }
            }
            base += slice.len();
        }

        if start < self.block_size() {
            let _ = f(LineSpan {
                index_num,
                block_start: start,
                block_end: self.block_size(),
                is_complete: false,
            });
        }
    }

    pub(crate) fn line_spans(&self) -> Vec<LineSpan> {
        let mut spans: Vec<LineSpan> = Vec::new();
        self.scan_line_spans(|line| {
            spans.push(line);
            true
        });
        spans
    }

    pub(crate) fn line_span_at_index(&self, line_idx: usize) -> Option<LineSpan> {
        let mut ret = None;
        self.scan_line_spans(|line| {
            if line.index_num == line_idx {
                ret = Some(line);
                false
            } else {
                true
            }
        });
        ret
    }

    pub(crate) fn line_span_at_offset(&self, block_offset: usize) -> Option<LineSpan> {
        let block_size = self.block_size();
        if block_size == 0 || block_offset >= block_size {
            return None;
        }
        let mut ret = None;
        self.scan_line_spans(|line| {
            if block_offset >= line.block_start && block_offset < line.block_end {
                ret = Some(line);
                false
            } else {
                true
            }
        });
        ret
    }

    pub(crate) fn first_line_span(&self) -> Option<LineSpan> {
        self.line_span_at_index(0)
    }

    pub(crate) fn last_line_span(&self) -> Option<LineSpan> {
        let mut ret = None;
        self.scan_line_spans(|line| {
            ret = Some(line);
            true
        });
        ret
    }

    pub(crate) fn split_offset_near_half(&self) -> usize {
        let half = self.block_size() / 2;
        let mut split_byte = None;
        self.scan_line_spans(|line| {
            if line.is_complete {
                let distance = (line.block_end as isize - half as isize).abs();
                match split_byte {
                    Some((best_distance, _)) if best_distance <= distance => {}
                    _ => split_byte = Some((distance, line.block_end)),
                }
            }
            true
        });
        split_byte.map(|(_, block_end)| block_end).unwrap_or(half)
    }
}

fn crc_checksum(data: &[u8]) -> u32 {
    let crc = Crc::<u32>::new(&CRC_32_ISO_HDLC);
    crc.checksum(data)
}
