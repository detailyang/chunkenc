# chunkenc
[ChunkEncoding](https://github.com/prometheus/prometheus/blob/0876d57aea9636898835c7a179760a6cd72a290d/tsdb/chunkenc/xor.go) is written in Rust to support chunk encoding.
> The original implement was written in Go via prometheus team

# use
```toml
chunkenc = {git = "https://github.com/detailyang/chunkenc.git", branch = "main"}
```

# usage
The `chunkenc` crates exists Iterator and Appender
```rust
use chunkenc::chunk::{Chunk, XORChunk};
fn main() {
    let mut chunk = XORChunk::new();
    let mut appender = chunk.appender().unwrap();
    {
        appender.append(1_i64, 2.0);
        appender.append(2_i64, 3.0);
        appender.append(3_i64, 4.0);
    }
    let mut it = chunk.iterator();
    {
        while it.next().unwrap_or_else(||false) {
            let (ts, val) = it.at();
        }
    }
}
```

# Disclaimer
The code within this repository comes with no guarantee, the use of this code is your responsibility.

I take NO responsibility and/or liability for how you choose to use any of the source code available here. By using any of the files available in this repository, you understand that you are AGREEING TO USE AT YOUR OWN RISK. Once again, ALL files available here are for EDUCATION and/or RESEARCH purposes ONLY.
