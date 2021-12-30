use thiserror::Error;
use unsigned_varint::io::ReadError;

#[derive(Error, Debug)]
pub enum Error {
    #[error("end of reader")]
    EOF,
    #[error("read error: {0}")]
    ReadError(ReadError),
    #[error("append over u16::MAX")]
    AppendOverflow,
}
