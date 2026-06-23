use crate::textwarp::ChapResult;
use crate::textwarp::GapBuffer;
use crate::textwarp::GapBytes;
use crate::textwarp::RingVec;
use crate::ChapError;
use crc::Crc;
use crc::CRC_32_ISO_HDLC;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::BufReader;
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
    pub(crate) logic_file_start: usize,  //块在文件逻辑开始位置
    pub(crate) source_file_start: usize, //块在原始 backing file 中的位置
    pub(crate) source_file_end: usize,   //原始块在文件结束位置 不能改变
    pub(crate) block_id: BlockId,        //块的稳定标识（分裂后不变）
    // start_line_index: usize,     //块内的起始行号在整个文件中
    pub(crate) line_count: usize,           //块内的行数
    pub(crate) logic_block_size: usize,     //逻辑块的大小
    pub(crate) lines_index: Vec<LineIndex>, //块内的行索引
    pub(crate) check_sum: u32,              //保存的时候会更新 sum
}

impl BlockIndex {
    pub(crate) fn file_end(&self) -> usize {
        self.logic_file_start + self.logic_block_size
    }

    pub(crate) fn source_file_end(&self) -> usize {
        self.source_file_end
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
        source_file_start: usize,
        source_file_end: usize,
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
            logic_file_start: file_start, //块在文件开始位置
            source_file_start: source_file_start,
            source_file_end: source_file_end,
            block_id: block_id,
            line_count: line_count,       //块内的行数
            logic_block_size: actual_len, //块的大小
            lines_index: lines_index,     //块内的行索引
            check_sum: sum,
        })
    }

    pub(crate) fn from_gap_slices(
        left: &[u8],
        right: &[u8],
        logic_file_start: usize,
        source_file_start: usize,
        source_file_end: usize,
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
            logic_file_start: logic_file_start,
            source_file_start: source_file_start,
            source_file_end: source_file_end,
            block_id,
            line_count,
            logic_block_size: actual_len,
            lines_index,
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
                valid_data,
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

//找机会重构
struct BlockManager {
    reader: BufReader<File>,
    blocks: RingVec<Block>,         // 每个块4KB大小（内存窗口）
    cache: HashMap<BlockId, Block>, // 缓存已修改的块（key = stable block_id）
    file_size: usize,               // 文件大小
    block_indexs: Vec<BlockIndex>,  // 每一块的索引（Vec位置是位置索引，block_id是稳定标识）
    next_id: BlockId,               // 下一个分配的 block_id
}

impl BlockManager {
    fn new(file: File) -> io::Result<Self> {
        let file_size = file.metadata()?.len() as usize;
        Ok(BlockManager {
            reader: BufReader::new(file),
            blocks: RingVec::with_capacity(BLOKK_NUM),
            cache: HashMap::new(),
            file_size,
            block_indexs: Vec::new(),
            next_id: 0,
        })
    }

    fn sort_loaded_blocks_by_logic_order(&mut self) {
        let order: HashMap<BlockId, usize> = self
            .block_indexs
            .iter()
            .enumerate()
            .map(|(pos, bi)| (bi.block_id, pos))
            .collect();
        self.blocks
            .sort_by_key(|b| order.get(&b.block_id).copied().unwrap_or(usize::MAX));
    }

    fn sync_block_source_mirror(
        block: &mut Block,
        source_file_start: usize,
        source_file_end: usize,
    ) {
        block.source_file_start = source_file_start;
        block.source_file_end = source_file_end;
    }

    fn cache_block_snapshot(&mut self, block: Block) {
        if block.is_modified {
            self.cache.insert(block.block_id, block);
        }
    }

    /// 设计说明：
    /// 块编辑后，`start_pos` 之后所有块的逻辑 `logic_file_start` 都可能发生连锁变化。
    /// 这里负责把 `block_indexs` 重新串成一个连续的逻辑视图，并重算 `file_size`。
    ///
    /// 关键点是：这里维护的是“逻辑偏移”，不是“原文件物理偏移”。
    /// 未加载块仍然保留各自的 `source_file_start`，以后如果真的访问到它们，再从原始 backing file
    /// 的对应位置读取。也正因为如此，这里不能像旧实现那样为了更新 offset 把后缀块全部 materialize：
    /// 那会让一次前部插入/删除触发整段后缀回读，既放大 IO，也破坏当前按需加载的设计。
    ///
    /// 换句话说，这个函数做的是：
    /// - 更新逻辑索引里的连续区间
    /// - 重新计算 `file_size`
    ///
    /// 它刻意不做的是：
    /// - 不回读未加载块
    /// - 不修改 `Block` 上的 `source_*` 冗余镜像
    fn sync_block_offsets_from(&mut self, start_pos: usize) {
        if self.block_indexs.is_empty() {
            self.file_size = 0;
            return;
        }
        let start_pos = start_pos.min(self.block_indexs.len().saturating_sub(1));
        let mut pos = if start_pos == 0 { 0 } else { start_pos };
        if pos > 0 {
            let prev = &self.block_indexs[pos - 1];
            let expected_start = prev.logic_file_start + prev.logic_block_size;
            if self.block_indexs[pos].logic_file_start != expected_start {
                self.block_indexs[pos].logic_file_start = expected_start;
            }
        }
        while pos < self.block_indexs.len() {
            if pos > 0 {
                let prev_end = self.block_indexs[pos - 1].logic_file_start
                    + self.block_indexs[pos - 1].logic_block_size;
                self.block_indexs[pos].logic_file_start = prev_end;
            }
            pos += 1;
        }
        self.file_size = self.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
    }

    /// 根据 block_id 查找其在 block_indexs 中的位置（O(n)，n 通常很小）
    fn block_pos(&self, block_id: BlockId) -> Option<usize> {
        if block_id == EMPTY_BLOCK_ID {
            return Some(0);
        }
        self.block_indexs
            .iter()
            .position(|bi| bi.block_id == block_id)
    }

    fn block_checksum(bytes: &[u8]) -> u32 {
        let crc = Crc::<u32>::new(&CRC_32_ISO_HDLC);
        crc.checksum(bytes)
    }

    fn block_checksum_slices(left: &[u8], right: &[u8]) -> u32 {
        let crc = Crc::<u32>::new(&CRC_32_ISO_HDLC);
        let mut digest = crc.digest();
        digest.update(left);
        digest.update(right);
        digest.finalize()
    }

    /// 分配新的 block_id（单调递增，分裂时使用）
    fn alloc_id(&mut self) -> BlockId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn split_block_once(&mut self, block_id: BlockId) -> ChapResult<Option<(BlockId, BlockId)>> {
        let pos = self.block_pos(block_id).ok_or_else(|| {
            ChapError::Unexpected(format!("split_block: block_id {} not found", block_id))
        })?;
        let logic_block_size = self.block_indexs[pos].logic_block_size;
        let logic_file_start = self.block_indexs[pos].logic_file_start;

        // ① 找切分点：完整行结尾里最接近逻辑块中点的那条；找不到时退化到中点切分
        let half = logic_block_size / 2;
        let split_byte = self.block_indexs[pos]
            .lines_index
            .iter()
            .filter(|l| l.is_complete)
            .min_by_key(|l| (l.block_end as isize - half as isize).abs())
            .map(|l| l.block_end)
            .unwrap_or(half);

        if split_byte == 0 || split_byte >= logic_block_size {
            return Ok(None); // 无法切分（整块只有一行且超大）
        }

        // ② 基于左右字节重建两个新块及其索引，保证超长行中间切分时 lines_index 也正确
        let (left_bytes, right_bytes, source_file_start, source_file_end) = {
            let block = self
                .blocks
                .iter_mut()
                .find(|b| b.block_id == block_id)
                .ok_or_else(|| {
                    ChapError::Unexpected(format!(
                        "split_block: block_id {} not in memory",
                        block_id
                    ))
                })?;
            let bytes = block.as_continuous();
            (
                bytes[..split_byte].to_vec(),
                bytes[split_byte..].to_vec(),
                block.source_file_start,
                block.source_file_end,
            )
        }; // self.blocks 借用在此结束
        let data_a = GapBuffer::from_bytes(&left_bytes, CHAR_GAP_SIZE);
        let data_b = GapBuffer::from_bytes(&right_bytes, CHAR_GAP_SIZE);
        let left_index = BlockIndex::from_block_bytes(
            &left_bytes,
            logic_file_start,
            source_file_start,
            source_file_end,
            block_id,
            left_bytes.len(),
            Self::block_checksum(&left_bytes),
        )?;

        // ④ 分配 block B 的新 block_id（单调递增，不影响其他块的 block_id）
        let new_block_id = self.alloc_id();
        let right_index = BlockIndex::from_block_bytes(
            &right_bytes,
            logic_file_start + split_byte,
            source_file_start,
            source_file_end,
            new_block_id,
            right_bytes.len(),
            Self::block_checksum(&right_bytes),
        )?;

        // 构造 Block B
        let block_b = Block {
            data: data_b,
            source_file_start: source_file_start,
            source_file_end: source_file_end,
            block_id: new_block_id,
            is_modified: true,
        };
        // 更新 Block A
        {
            let block = self
                .blocks
                .iter_mut()
                .find(|b| b.block_id == block_id)
                .unwrap();
            block.data = data_a;
            Self::sync_block_source_mirror(block, source_file_start, source_file_end);
            block.is_modified = true;
        }

        // ⑤ 更新 block_indexs：替换 A，插入 B（不需要重编号后续块！）
        self.block_indexs[pos] = left_index;
        self.block_indexs.insert(pos + 1, right_index);
        // ⑥ cache 以 block_id 为 key，其他块 block_id 未变，无需更新

        // ⑦ push block_b；被挤出时存入 cache（key = block_id）
        if let Some(evicted) = self.blocks.push(block_b) {
            self.cache_block_snapshot(evicted);
        }
        // 分裂后的子块共享同一 source_* 区间，已不能再靠 source_file_start 推导文本顺序。
        self.sort_loaded_blocks_by_logic_order();

        Ok(Some((block_id, new_block_id)))
    }

    fn split_block(&mut self, block_id: BlockId) -> ChapResult<()> {
        let Some(pos) = self.block_pos(block_id) else {
            return Err(ChapError::Unexpected(format!(
                "split_block: block_id {} not found",
                block_id
            )));
        };
        self.sync_block_offsets_from(pos);
        let mut pending = vec![block_id];
        while let Some(next_id) = pending.pop() {
            let Some(next_pos) = self.block_pos(next_id) else {
                continue;
            };
            if self.block_indexs[next_pos].logic_block_size <= BLOCK_SIZE {
                continue;
            }
            if let Some((left_id, right_id)) = self.split_block_once(next_id)? {
                pending.push(right_id);
                pending.push(left_id);
            }
        }
        if !self.block_indexs.is_empty() {
            self.sync_block_offsets_from(pos);
        }
        // self.debug_assert_storage_consistent();
        Ok(())
    }

    #[cfg(debug_assertions)]
    fn debug_assert_storage_consistent(&self) {
        let total: usize = self.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        debug_assert_eq!(
            total, self.file_size,
            "sum(logic_block_size) must equal file_size"
        );
        for (i, bi) in self.block_indexs.iter().enumerate() {
            if i > 0 {
                let prev = &self.block_indexs[i - 1];
                debug_assert_eq!(
                    bi.logic_file_start,
                    prev.logic_file_start + prev.logic_block_size,
                    "block {} logic_file_start must be contiguous",
                    i
                );
            }
            let complete_count = bi.lines_index.iter().filter(|l| l.is_complete).count();
            debug_assert_eq!(
                bi.line_count, complete_count,
                "block {} line_count must match complete lines",
                i
            );
            let mut expected_start = 0usize;
            for (line_idx, line) in bi.lines_index.iter().enumerate() {
                debug_assert_eq!(
                    line.block_start, expected_start,
                    "block {} line {} start must be contiguous",
                    i, line_idx
                );
                debug_assert!(
                    line.block_start <= line.block_end && line.block_end <= bi.logic_block_size,
                    "block {} line {} range must be within block",
                    i,
                    line_idx
                );
                expected_start = line.block_end;
            }
            if let Some(block) = self.blocks.iter().find(|b| b.block_id == bi.block_id) {
                debug_assert_eq!(block.source_file_start, bi.source_file_start);
                debug_assert_eq!(block.source_file_end, bi.source_file_end);
                debug_assert_eq!(block.block_size(), bi.logic_block_size);
            }
        }
    }
}
