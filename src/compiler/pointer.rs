use serde_json::{Value, json};

const HEX: &[u8; 16] = b"0123456789ABCDEF";

pub(super) fn path(key: &str) -> String {
    let pointer = key.replace('~', "~0").replace('/', "~1");
    let mut encoded = String::with_capacity(pointer.len());

    for byte in pointer.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }

    format!("#/$defs/{encoded}")
}

pub(super) fn reference(key: &str) -> Value {
    json!({ "$ref": path(key) })
}
