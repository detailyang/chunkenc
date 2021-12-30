use unsigned_varint::encode;
use unsigned_varint::io;

pub(crate) fn encode_i64(number: i64, buf: &mut [u8; 10]) -> &[u8] {
    let mut ux = (number as u64) << 1;
    if number < 0 {
        ux = !ux;
    }
    encode::u64(ux, buf)
}

pub(crate) fn read_i64<R: std::io::Read>(r: R) -> Result<i64, crate::error::Error> {
    let ux = io::read_u64(r).map_err(crate::error::Error::ReadError)?;
    let mut x = (ux >> 1) as i64;
    if ux & 1 != 0 {
        x = !x;
    }
    Ok(x)
}
