// use crate::fuzzy::boyermoore::BoyerMoore;

// use crate::tui::TextSelect;

// use inherit_methods_macro::inherit_methods;
// use memmap2::Mmap;
// use mlua::Either;
// use std::borrow::Cow;
// use std::cell::UnsafeCell;
// use std::collections::HashMap;
// use std::fs;
// use std::fs::File;
// use std::io::{BufRead, BufReader, Read, Seek, Write};
// use std::ops::Bound;
// use std::ops::RangeBounds;
// use std::path::{Path, PathBuf};
// use std::ptr::NonNull;
// use unicode_width::UnicodeWidthChar;
// use utf8_iter::Utf8CharIndices;
// use utf8_iter::Utf8CharsEx;

// pub(crate) trait Line {
//     fn text_len(&self) -> usize;
//     fn text(&self, range: impl RangeBounds<usize>) -> GapBytes;
// }

// pub(crate) struct FileIoText {
//     file: BufReader<File>,
// }

// impl FileIoText {
//     pub(crate) fn from_file_path<P: AsRef<Path>>(filename: P) -> ChapResult<FileIoText> {
//         let file = File::open(filename)?;
//         Ok(FileIoText {
//             file: BufReader::new(file),
//         })
//     }
// }

// pub struct FileIoTextIter<'a> {
//     file: &'a BufReader<File>,
//     line_index: usize,
//     line_file_start: usize,
//     line_file_end: usize,
// }

// #[cfg(test)]
// mod tests {

//     use super::*;
//     use crate::fuzzy::boyermoore::BoyerMoore;
//     use ratatui::text;

//     #[test]
//     fn test_item_parser() {
//         let word: u32 = 0x005A9F10;

//         // lp_len 是最低 15 位
//         let lp_len = (word & 0x7FFF) as u16;
//         // lp_flags 接着的 2 位
//         let lp_flags = ((word >> 15) & 0x3) as u8;
//         // lp_off 再上面的 15 位
//         let lp_off = ((word >> 17) & 0x7FFF) as u16;
//         println!(
//             "lp_len: {}, lp_flags: {}, lp_off: {}",
//             lp_len, lp_flags, lp_off
//         );
//     }

//     #[test]
//     fn test_hex_u8_iter() {
//         let mut hex_text = HexText::from_file_path("/root/20250704120009_481.jpg", 48).unwrap();
//         let hex_u8_iter = hex_text.iter_u8(0, 0, 0);
//         for (i, u8) in hex_u8_iter.enumerate() {
//             println!("u8: {:x},i: {}", u8, i);
//         }
//     }

//     #[test]
//     fn test_hex_u8_iter_search() {
//         let mut hex_text = HexText::from_file_path("/root/20250704120009_481.jpg", 48).unwrap();
//         // let hex_u8_iter = hex_text.iter_u8(0, 0, 0);
//         // let pattern: Vec<u8> = vec![0x27, 0x96, 0xA7];
//         // let bm = BoyerMoore::new(pattern.as_slice());
//         // for i in bm.stream(hex_u8_iter) {
//         //     println!("{}", i);
//         // }

//         let hex_u8_iter = hex_text.iter_u8(0, 0, 0);
//         let pattern = "46546767676713454464654";
//         let bm = BoyerMoore::new(pattern.as_bytes());
//         for i in bm.stream(hex_u8_iter) {
//             println!("{}", i);
//         }
//     }

//     #[test]
//     fn text_hex_get() {
//         let hex_text = TextWarp::new(
//             HexText::from_file_path("/root/20250704120009_481.jpg", 48).unwrap(),
//             48,
//             0,
//             TextWarpType::NoWrap,
//         );
//         let line = hex_text.get_one_page(1).unwrap();
//         let line = hex_text.get_one_page(93).unwrap();
//         println!("=======================================");
//         for s in line.0.iter() {
//             let (a, b) = s.as_slice();
//             for c in a.iter() {
//                 print!("{:02x} ", c);
//             }
//             print!("\n");
//         }
//     }

//     #[test]
//     fn text_hex_sel() {
//         let mut hex = HexText::from_file_path("/root/20250704120009_481.jpg", 48).unwrap();

//         let sel = TextSelect::from_select(100, 1000);
//         for s in hex.chunks.iter() {
//             println!("chunk: {:?}", s.buffer.text(..).to_vec());
//         }
//         let text = hex.text_from_sel(&sel);
//         println!("text1 len  {:?}", text.len());
//         println!("text1  {:?}\n", text);

//         let sel = TextSelect::from_select(0, 3);
//         for s in hex.chunks.iter() {
//             println!("chunk: {:?}", s.buffer.text(..).to_vec());
//         }
//         let text = hex.text_from_sel(&sel);
//         println!("text2 len  {:?}", text.len());
//         println!("text2  {:?}\n", text);

//         let sel = TextSelect::from_select(2, 6);
//         for s in hex.chunks.iter() {
//             println!("chunk: {:?}", s.buffer.text(..).to_vec());
//         }
//         let text = hex.text_from_sel(&sel);
//         println!("text3 len  {:?}", text.len());
//         println!("text3  {:?}\n", text);

//         let sel = TextSelect::from_select(8, 9);
//         for s in hex.chunks.iter() {
//             println!("chunk: {:?}", s.buffer.text(..).to_vec());
//         }
//         let text = hex.text_from_sel(&sel);
//         println!("text4 len  {:?}", text.len());
//         println!("text4  {:?}\n", text);

//         let sel = TextSelect::from_select(0, 0);
//         for s in hex.chunks.iter() {
//             println!("chunk: {:?}", s.buffer.text(..).to_vec());
//         }
//         let text = hex.text_from_sel(&sel);
//         println!("text5 len  {:?}", text.len());
//         println!("text5  {:?}\n", text);
//     }

//     #[test]
//     fn text_hex() {
//         let mut hex = HexText::from_file_path("/root/aa.txt", 48).unwrap();
//         println!("hex len: {:?}", hex.chunks.get(0).unwrap().buffer.text(..));
//         let iter = hex.iter(0, 10, 0);
//         for i in iter {
//             println!("i: {:?}", i.line_data.as_str());
//         }
//     }

//     #[test]
//     fn test_print() {
//         let file = File::open("/root/aa.txt").unwrap();
//         let mmap = unsafe { Mmap::map(&file).unwrap() };
//         println!(" mmap len: {:?}", mmap.len());
//         let mmap_text = MmapText::new(mmap);

//         let text = TextWarp::new(mmap_text, 2, 5, TextWarpType::NoWrap);
//         let (s, c) = text.get_one_page(1).unwrap();
//         for (i, l) in s.iter().enumerate() {
//             println!("l: {:?},{:?}", l.as_str(), c.get(i));
//         }

//         // for p in text.borrow_page_offset_list().iter() {
//         //     println!("p:{:?}", p)
//         // }

//         // let (s, c) = text.get_next_line(c.last().unwrap(), 1);
//         // println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);

//         // let (s, c) = text.get_next_line(&c, 1);
//         // println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
//         // let (s, c) = text.get_next_line(&c, 1);
//         // println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
//         // let (s, c) = text.get_next_line(&c, 1);
//         // println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
//         // let (s, c) = text.get_next_line(&c, 1);
//         // println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
//         // let (s, c) = text.get_next_line(&c, 1);
//         // println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
//         // let (s, c) = text.get_next_line(&c, 1);
//         // println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
//         // let (s, c) = text.get_next_line(&c, 1);
//         // println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
//         // let (s, c) = text.get_next_line(&c, 1);
//         // println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
//         // let (s, c) = text.get_next_line(&c, 1);
//         // println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
//         // let (s, c) = text.get_next_line(&c, 1);
//         // println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
//         // let (s, c) = text.get_next_line(&c, 1);
//         // println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
//         // let (s, c) = text.get_next_line(&c, 1);
//         // println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
//     }

//     #[test]
//     fn test_ringcache() {
//         let mut ring_cache = RingVec::<usize>::new(8);
//         for i in 0..11 {
//             ring_cache.push(i);
//         }

//         for i in ring_cache.iter() {
//             println!("i: {:?}", i);
//         }

//         println!("{:?}", ring_cache.cache);

//         ring_cache.push_front(0);

//         for i in ring_cache.iter() {
//             println!("i1: {:?}", i);
//         }

//         ring_cache.push_front(11);
//         ring_cache.push_front(12);
//         ring_cache.push_front(13);
//         ring_cache.push_front(14);
//         ring_cache.push_front(15);
//         ring_cache.push_front(16);
//         ring_cache.push_front(17);
//         ring_cache.push_front(18);
//         ring_cache.push_front(19);

//         for i in ring_cache.iter() {
//             println!("i2: {:?}", i);
//         }

//         println!("{:?}", ring_cache.get(0));
//         println!("{:?}", ring_cache.get(1));
//         println!("{:?}", ring_cache.get(2));
//         println!("{:?}", ring_cache.get(3));
//         println!("{:?}", ring_cache.get(4));
//         println!("{:?}", ring_cache.get(5));
//         println!("{:?}", ring_cache.get(6));
//         println!("{:?}", ring_cache.get(7));
//         println!("{:?}", ring_cache.get(8));
//         println!("{:?}", ring_cache.get(9));
//         println!("{:?}", ring_cache.get(10));
//         println!("{:?}", ring_cache.get(11));

//         ring_cache.remove(0);

//         for i in ring_cache.iter() {
//             println!("i3: {:?}", i);
//         }

//         ring_cache.push(20);

//         for i in ring_cache.iter() {
//             println!("i4: {:?}", i);
//         }

//         // println!("{:?}", ring_cache.last());
//     }
//     #[test]
//     fn test_remove() {
//         let mut ring_cache = RingVec::<usize>::new(8);
//         for i in 0.. {
//             ring_cache.push(i);
//         }

//         for i in ring_cache.iter() {
//             println!("i: {:?}", i);
//         }

//         println!("{:?}", ring_cache.cache);
//         ring_cache.remove(0);

//         for i in ring_cache.iter() {
//             println!("i: {:?}", i);
//         }

//         println!("{:?}", ring_cache.cache);

//         ring_cache.push(20);
//         for i in ring_cache.iter() {
//             println!("i: {:?}", i);
//         }
//         println!("{:?}", ring_cache.cache);

//         ring_cache.remove(0);

//         for i in ring_cache.iter() {
//             println!("i: {:?}", i);
//         }

//         println!("{:?}", ring_cache.cache);

//         ring_cache.push(30);
//         for i in ring_cache.iter() {
//             println!("i: {:?}", i);
//         }
//         println!("{:?}", ring_cache.cache);
//     }

//     #[test]
//     fn test_remove2() {
//         let mut ring_cache = RingVec::<usize>::new(3);
//         for i in 0..3 {
//             ring_cache.push(i);
//         }

//         for i in ring_cache.iter().enumerate() {
//             println!("i: {:?}", i);
//         }

//         println!("{:?},{}", ring_cache.cache, ring_cache.start);
//         ring_cache.remove(7);

//         for i in ring_cache.iter().enumerate() {
//             println!("i: {:?}", i);
//         }

//         println!("{:?},{}", ring_cache.cache, ring_cache.start);

//         ring_cache.push_front(20);
//         for i in ring_cache.iter().enumerate() {
//             println!("i: {:?}", i);
//         }
//         println!("push_front {:?}", ring_cache.cache);

//         ring_cache.remove(7);

//         for i in ring_cache.iter().enumerate() {
//             println!("i: {:?}", i);
//         }

//         println!("{:?},{}", ring_cache.cache, ring_cache.start);

//         ring_cache.push_front(30);
//         for i in ring_cache.iter().enumerate() {
//             println!("i: {:?}", i);
//         }

//         ring_cache.remove(7);
//         ring_cache.push_front(40);

//         for i in ring_cache.iter().enumerate() {
//             println!("i: {:?}", i);
//         }

//         println!("push_front{:?}", ring_cache.cache);

//         ring_cache.remove(7);
//         ring_cache.push_front(50);

//         for i in ring_cache.iter().enumerate() {
//             println!("i: {:?}", i);
//         }

//         println!("push_front{:?}", ring_cache.cache);
//     }

//     // #[test]
//     // fn test_print3() {
//     //     let mut b = EditTextBuffer::from_file_path("/root/aa.txt", 2, 5).unwrap();

//     //     let c = {
//     //         let (s, c) = b.get_line_content_with_count(1, 11);
//     //         for (i, l) in s.iter().enumerate() {
//     //             println!("l: {:?}{:?}", l.as_str(), c.get(i).unwrap());
//     //         }

//     //         for p in b.borrow_page_offset_list().iter() {
//     //             println!("p:{:?}", p)
//     //         }
//     //         c
//     //     };
//     //     let (s, m) = b.get_pre_line(&c.last().unwrap(), 1);
//     //     println!("{:?},{:?}", s.unwrap().as_str(), m);
//     //     let (s, m) = b.get_pre_line(&m, 1);
//     //     println!("{:?},{:?}", s.unwrap().as_str(), m);
//     //     let (s, m) = b.get_pre_line(&m, 1);
//     //     println!("{:?},{:?}", s.unwrap().as_str(), m);
//     //     let (s, m) = b.get_pre_line(&m, 1);
//     //     println!("{:?},{:?}", s.unwrap().as_str(), m);
//     //     let (s, m) = b.get_pre_line(&m, 1);
//     //     println!("{:?},{:?}", s.unwrap().as_str(), m);
//     //     let (s, m) = b.get_pre_line(&m, 1);
//     //     println!("{:?},{:?}", s.unwrap().as_str(), m);
//     //     let (s, m) = b.get_pre_line(&m, 1);
//     //     println!("{:?},{:?}", s.unwrap().as_str(), m);
//     //     let (s, m) = b.get_pre_line(&m, 1);
//     //     println!("{:?},{:?}", s.unwrap().as_str(), m);
//     //     let (s, m) = b.get_pre_line(&m, 1);
//     //     println!("{:?},{:?}", s.unwrap().as_str(), m);
//     //     let (s, m) = b.get_pre_line(&m, 1);
//     //     println!("{:?},{:?}", s.unwrap().as_str(), m);
//     //     let (s, m) = b.get_pre_line(&m, 1);
//     //     println!("{:?},{:?}", s.unwrap().as_str(), m);
//     //     let (s, m) = b.get_pre_line(&m, 1);
//     //     println!("{:?},{:?}", s.unwrap().as_str(), m);
//     //     let (s, m) = b.get_pre_line(&m, 1);
//     //     println!("{:?},{:?}", s.unwrap().as_str(), m);
//     //     let (s, m) = b.get_pre_line(&m, 1);
//     //     println!("{:?},{:?}", s.unwrap().as_str(), m);
//     //     let (s, m) = b.get_pre_line(&m, 1);
//     //     println!("{:?},{:?}", s.unwrap().as_str(), m);

//     //     // for p in b.borrow_page_offset_list().iter() {
//     //     //     println!("p:{:?}", p)
//     //     // }
//     // }

//     // #[test]
//     // fn test_print4() {
//     //     let mut b = EditTextBuffer::from_file_path("/root/aa.txt", 2, 5).unwrap();

//     //     let c = {
//     //         let (s, c) = b.get_line_content_with_count(1, 11);
//     //         for (i, l) in s.iter().enumerate() {
//     //             println!("l0: {:?}{:?}", l.as_str(), c.get(i).unwrap());
//     //         }

//     //         let (s, c) = b.get_line_content_with_count(4, 11);
//     //         for (i, l) in s.iter().enumerate() {
//     //             println!("l1: {:?}{:?}", l.as_str(), c.get(i).unwrap());
//     //         }

//     //         for p in b.borrow_page_offset_list().iter() {
//     //             println!("p:{:?}", p)
//     //         }
//     //         c
//     //     };
//     // }

//     #[test]
//     fn test_mmap() {
//         let path = "/root/aa.txt";
//         let file = std::fs::File::open(path).unwrap();
//         let mmap = unsafe { Mmap::map(&file).unwrap() };
//         //let
//     }
// }
