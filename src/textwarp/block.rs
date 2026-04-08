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
//一行数据
pub(crate) struct LineIndex {
    pub(crate) index_num: usize,   //行号
    pub(crate) block_start: usize, //行在块开始位置
    pub(crate) block_end: usize,   //行在块结束位置
    pub(crate) is_complete: bool,  //是否完整行
}

impl LineIndex {
    fn line_size(&self) -> usize {
        self.block_end - self.block_start
    }
}

#[derive(Clone)]
pub(crate) struct BlockIndex {
    pub(crate) file_start: usize,        //块在文件开始位置
    pub(crate) source_file_start: usize, //块在原始 backing file 中的位置
    pub(crate) block_id: BlockId,        //块的稳定标识（分裂后不变）
    // start_line_index: usize,     //块内的起始行号在整个文件中
    pub(crate) line_count: usize,           //块内的行数
    pub(crate) block_size: usize,           //块的大小
    pub(crate) lines_index: Vec<LineIndex>, //块内的行索引
    pub(crate) check_sum: u32,              //保存的时候会更新 sum
}

impl BlockIndex {
    pub(crate) fn file_end(&self) -> usize {
        self.file_start + self.block_size
    }

    pub(crate) fn get_line_index(&self, block_offset: usize) -> Option<&LineIndex> {
        for l in self.lines_index.iter() {
            if block_offset >= l.block_start && block_offset < l.block_end {
                return Some(l);
            }
        }
        None
    }

    pub(crate) fn from_block_bytes(
        valid_data: &[u8],
        file_start: usize,
        block_id: BlockId,
        actual_len: usize,
        sum: u32,
    ) -> ChapResult<BlockIndex> {
        let mut lines_index = Vec::new();
        let mut block_offset: usize = 0;
        let mut line_count: usize = 0;
        let mut index_num: usize = 0;
        for (idx, &byte) in valid_data.iter().enumerate() {
            if byte != b'\n' {
                continue;
            }
            let end = idx + 1;
            lines_index.push(LineIndex {
                index_num,
                is_complete: true,
                block_start: block_offset,
                block_end: end,
            });
            line_count += 1;
            block_offset = end;
            index_num += 1;
        }

        if block_offset < actual_len {
            lines_index.push(LineIndex {
                index_num,
                is_complete: false,
                block_start: block_offset,
                block_end: actual_len,
            });
        }

        Ok(BlockIndex {
            file_start: file_start, //块在文件开始位置
            source_file_start: file_start,
            block_id: block_id,
            line_count: line_count,   //块内的行数
            block_size: actual_len,   //块的大小
            lines_index: lines_index, //块内的行索引
            check_sum: sum,
        })
    }

    pub(crate) fn from_gap_slices(
        left: &[u8],
        right: &[u8],
        file_start: usize,
        block_id: BlockId,
        actual_len: usize,
        sum: u32,
    ) -> ChapResult<BlockIndex> {
        let mut lines_index = Vec::new();
        let mut line_start: usize = 0;
        let mut line_count: usize = 0;
        let mut index_num: usize = 0;

        for (offset, &byte) in left.iter().chain(right.iter()).enumerate() {
            if byte == b'\n' {
                lines_index.push(LineIndex {
                    index_num,
                    is_complete: true,
                    block_start: line_start,
                    block_end: offset + 1,
                });
                line_count += 1;
                index_num += 1;
                line_start = offset + 1;
            }
        }

        if line_start < actual_len {
            lines_index.push(LineIndex {
                index_num,
                is_complete: false,
                block_start: line_start,
                block_end: actual_len,
            });
        }

        Ok(BlockIndex {
            file_start,
            source_file_start: file_start,
            block_id,
            line_count,
            block_size: actual_len,
            lines_index,
            check_sum: sum,
        })
    }
}

#[derive(Clone)]
//按块加载文件 每个块4KB大小
pub(crate) struct Block {
    pub(crate) data: GapBuffer,   //每一个块使用 GapBuffer 存储
    pub(crate) file_start: usize, //块在文件开始位置
    pub(crate) file_end: usize,   //块在文件结束位置
    pub(crate) block_id: BlockId, //块的稳定标识（分裂后不变）
    pub(crate) is_modified: bool, //块是否被修改
}

impl Block {
    pub(crate) fn text_from_line(&self, index: &LineIndex) -> GapBytes<'_> {
        self.text(index.block_start..index.block_end)
    }

    fn text(&self, block_range: impl std::ops::RangeBounds<usize>) -> GapBytes<'_> {
        self.data.text(block_range)
    }

    pub(crate) fn text_from_file_seek(
        &self,
        range: impl std::ops::RangeBounds<usize>,
    ) -> GapBytes<'_> {
        let start = match range.start_bound() {
            std::ops::Bound::Included(&start) => start,
            std::ops::Bound::Excluded(&start) => start + 1,
            std::ops::Bound::Unbounded => self.file_start,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(&end) => end, //包含
            std::ops::Bound::Excluded(&end) => end, //排除
            std::ops::Bound::Unbounded => self.file_end,
        };
        assert!(start <= end && start >= self.file_start && end >= self.file_start);
        self.data
            .text((start - self.file_start)..(end - self.file_start))
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
        // buf.clear();
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
            let block_index =
                BlockIndex::from_block_bytes(valid_data, file_start, block_id, actual_len, sum)?;
            Some(block_index)
        } else {
            None
        };

        let block = Block {
            data: GapBuffer::from_bytes(valid_data, CHAR_GAP_SIZE),
            file_start: file_start,
            file_end: file_start + actual_len,
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
        self.is_modified = true;
    }

    pub(crate) fn block_size(&self) -> usize {
        self.data.text_len()
    }
}

fn crc_checksum(data: &[u8]) -> u32 {
    let crc = Crc::<u32>::new(&CRC_32_ISO_HDLC);
    crc.checksum(data)
}
