//! JPEG and JPEG 2000 passthrough planning.

pub(crate) fn j2k_codestream_is_rpcl(data: &[u8]) -> bool {
    const MARKER_SOC: u16 = 0xFF4F;
    const MARKER_COD: u16 = 0xFF52;
    const MARKER_SOT: u16 = 0xFF90;
    const MARKER_SOD: u16 = 0xFF93;
    const MARKER_EOC: u16 = 0xFFD9;
    const PROGRESSION_RPCL: u8 = 2;

    let mut offset = 0usize;
    if read_be_u16(data, offset) == Some(MARKER_SOC) {
        offset += 2;
    }
    while offset + 4 <= data.len() {
        while offset < data.len() && data[offset] == 0xFF {
            offset += 1;
        }
        if offset >= data.len() {
            return false;
        }
        let marker = 0xFF00 | u16::from(data[offset]);
        offset += 1;
        match marker {
            MARKER_COD => {
                let Some(length) = read_be_u16(data, offset).map(usize::from) else {
                    return false;
                };
                if length < 4 || offset + length > data.len() {
                    return false;
                }
                let payload = &data[offset + 2..offset + length];
                return payload.get(1) == Some(&PROGRESSION_RPCL);
            }
            MARKER_SOT | MARKER_SOD | MARKER_EOC => return false,
            _ => {
                let Some(length) = read_be_u16(data, offset).map(usize::from) else {
                    return false;
                };
                if length < 2 || offset + length > data.len() {
                    return false;
                }
                offset += length;
            }
        }
    }
    false
}

fn read_be_u16(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}
