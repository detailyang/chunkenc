//! [ChunkEncoding](https://github.com/prometheus/prometheus/blob/0876d57aea9636898835c7a179760a6cd72a290d/tsdb/chunkenc/xor.go) written in Rust
//!
//! The `chunkenc` crates exists Iterator and Appender
//!
//! # Example
//!
//! ```
//! use chunkenc::chunk::{Chunk, XORChunk};
//!
//! fn main() {
//!     let mut chunk = XORChunk::new();
//!     let mut appender = chunk.appender().unwrap();
//!     {
//!         appender.append(1_i64, 2.0);
//!         appender.append(2_i64, 3.0);
//!         appender.append(3_i64, 4.0);
//!     }
//!     let mut it = chunk.iterator();
//!     {
//!         while it.next().unwrap_or_else(||false) {
//!             let (ts, val) = it.at();
//!         }
//!     }
//! }

pub mod bitstream;
pub mod chunk;
pub mod error;
mod helper;
