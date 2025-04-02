use memmap2::Mmap;
use std::fs::File;
use std::io;
use std::io::BufRead;
use std::path::Path;
// 使用 mmap 映射文件到内存
pub(crate) fn map_file(path: &str) -> io::Result<Mmap> {
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
