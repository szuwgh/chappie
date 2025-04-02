# Diff Details

Date : 2025-03-23 13:33:19

Directory /opt/rsproject/chappie/crates/galois/src

Total : 37 files,  -31 codes, 1645 comments, 178 blanks, all 1792 lines

[Summary](results.md) / [Details](details.md) / [Diff Summary](diff.md) / Diff Details

## Files
| filename | language | code | comment | blank | total |
| :--- | :--- | ---: | ---: | ---: | ---: |
| [crates/galois/src/cuda.rs](/crates/galois/src/cuda.rs) | Rust | 1,649 | 14 | 137 | 1,800 |
| [crates/galois/src/error.rs](/crates/galois/src/error.rs) | Rust | 82 | 12 | 11 | 105 |
| [crates/galois/src/ggml\_quants.rs](/crates/galois/src/ggml_quants.rs) | Rust | 694 | 10 | 94 | 798 |
| [crates/galois/src/kernels.rs](/crates/galois/src/kernels.rs) | Rust | 21 | 0 | 2 | 23 |
| [crates/galois/src/lib.rs](/crates/galois/src/lib.rs) | Rust | 2,277 | 589 | 396 | 3,262 |
| [crates/galois/src/macros.rs](/crates/galois/src/macros.rs) | Rust | 10 | 0 | 1 | 11 |
| [crates/galois/src/op.rs](/crates/galois/src/op.rs) | Rust | 1,663 | 1,513 | 487 | 3,663 |
| [crates/galois/src/shape.rs](/crates/galois/src/shape.rs) | Rust | 464 | 61 | 91 | 616 |
| [crates/galois/src/simd.rs](/crates/galois/src/simd.rs) | Rust | 0 | 0 | 2 | 2 |
| [crates/galois/src/similarity.rs](/crates/galois/src/similarity.rs) | Rust | 164 | 8 | 26 | 198 |
| [crates/galois/src/zip.rs](/crates/galois/src/zip.rs) | Rust | 89 | 12 | 17 | 118 |
| [crates/vectorbase/src/ann/annoy.rs](/crates/vectorbase/src/ann/annoy.rs) | Rust | -6 | -34 | -6 | -46 |
| [crates/vectorbase/src/ann/hnsw.rs](/crates/vectorbase/src/ann/hnsw.rs) | Rust | -458 | -196 | -64 | -718 |
| [crates/vectorbase/src/ann/mod.rs](/crates/vectorbase/src/ann/mod.rs) | Rust | -131 | -21 | -22 | -174 |
| [crates/vectorbase/src/buffer.rs](/crates/vectorbase/src/buffer.rs) | Rust | -521 | 0 | -64 | -585 |
| [crates/vectorbase/src/collection.rs](/crates/vectorbase/src/collection.rs) | Rust | -423 | -21 | -46 | -490 |
| [crates/vectorbase/src/compaction.rs](/crates/vectorbase/src/compaction.rs) | Rust | -60 | 0 | -5 | -65 |
| [crates/vectorbase/src/config.rs](/crates/vectorbase/src/config.rs) | Rust | -176 | 0 | -34 | -210 |
| [crates/vectorbase/src/disk.rs](/crates/vectorbase/src/disk.rs) | Rust | -1,183 | -79 | -165 | -1,427 |
| [crates/vectorbase/src/lib.rs](/crates/vectorbase/src/lib.rs) | Rust | -1,463 | -102 | -237 | -1,802 |
| [crates/vectorbase/src/macros.rs](/crates/vectorbase/src/macros.rs) | Rust | -17 | 0 | -2 | -19 |
| [crates/vectorbase/src/query.rs](/crates/vectorbase/src/query.rs) | Rust | -72 | 0 | -16 | -88 |
| [crates/vectorbase/src/schema.rs](/crates/vectorbase/src/schema.rs) | Rust | -1,145 | -17 | -161 | -1,323 |
| [crates/vectorbase/src/searcher.rs](/crates/vectorbase/src/searcher.rs) | Rust | -130 | 0 | -24 | -154 |
| [crates/vectorbase/src/sql.rs](/crates/vectorbase/src/sql.rs) | Rust | -1 | 0 | -1 | -2 |
| [crates/vectorbase/src/task.rs](/crates/vectorbase/src/task.rs) | Rust | -2 | 0 | -2 | -4 |
| [crates/vectorbase/src/tokenize.rs](/crates/vectorbase/src/tokenize.rs) | Rust | -8 | 0 | -4 | -12 |
| [crates/vectorbase/src/util/asyncio.rs](/crates/vectorbase/src/util/asyncio.rs) | Rust | -198 | -8 | -37 | -243 |
| [crates/vectorbase/src/util/bloom.rs](/crates/vectorbase/src/util/bloom.rs) | Rust | -105 | -1 | -21 | -127 |
| [crates/vectorbase/src/util/common.rs](/crates/vectorbase/src/util/common.rs) | Rust | -100 | -9 | -15 | -124 |
| [crates/vectorbase/src/util/error.rs](/crates/vectorbase/src/util/error.rs) | Rust | -118 | 0 | -13 | -131 |
| [crates/vectorbase/src/util/fs.rs](/crates/vectorbase/src/util/fs.rs) | Rust | -406 | -20 | -65 | -491 |
| [crates/vectorbase/src/util/fst.rs](/crates/vectorbase/src/util/fst.rs) | Rust | -94 | -34 | -25 | -153 |
| [crates/vectorbase/src/util/index.rs](/crates/vectorbase/src/util/index.rs) | Rust | -25 | -20 | -6 | -51 |
| [crates/vectorbase/src/util/mod.rs](/crates/vectorbase/src/util/mod.rs) | Rust | -8 | 0 | -1 | -9 |
| [crates/vectorbase/src/util/time.rs](/crates/vectorbase/src/util/time.rs) | Rust | -7 | 0 | -3 | -10 |
| [crates/vectorbase/src/wal.rs](/crates/vectorbase/src/wal.rs) | Rust | -287 | -12 | -47 | -346 |

[Summary](results.md) / [Details](details.md) / [Diff Summary](diff.md) / Diff Details