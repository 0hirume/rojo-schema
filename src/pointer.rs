use serde_json::{json, Value};

const HEX: &[u8; 16] = b"0123456789ABCDEF";

pub fn path(key: &str) -> String {
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

pub fn reference(key: &str) -> Value {
    json!({ "$ref": path(key) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_json_pointer_as_uri_fragment() {
        assert_eq!(
            path("property/PVInstance/Pivot Offset"),
            "#/$defs/property~1PVInstance~1Pivot%20Offset"
        );
    }
}
