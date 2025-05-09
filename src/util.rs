use memmap2::Mmap;
use std::fs::File;
use std::io;
use std::io::BufRead;
use std::path::Path;
// 使用 mmap 映射文件到内存
pub(crate) fn mmap_file<P: AsRef<Path>>(path: P) -> io::Result<Mmap> {
    let file = File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    Ok(mmap)
}

/// 读取文件的每一行
pub(crate) fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<File>>>
where
    P: AsRef<Path>,
{
    // 打开文件
    let file = File::open(filename)?;
    // 创建 BufReader 并返回行迭代器
    Ok(io::BufReader::new(file).lines())
}

pub(crate) fn get_char_byte_len(c: char) -> usize {
    // 计算字符的字节长度
    if c == char::REPLACEMENT_CHARACTER {
        return 1;
    }
    c.len_utf8()
}
