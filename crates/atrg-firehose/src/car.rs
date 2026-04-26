//! Minimal CAR v1 file decoder for extracting CBOR blocks.
//!
//! The firehose `#commit` frames include a `blocks` byte array that is
//! a CAR v1 archive. We decode it to extract the record CBOR blocks,
//! converting each from CBOR (via `ciborium`) into `serde_json::Value`.

use std::collections::HashMap;

/// Decoded blocks from a CAR v1 file, keyed by hex-encoded CID.
#[derive(Debug)]
pub struct CarDecoded {
    /// Map from CID (hex-encoded) to decoded CBOR block value.
    pub blocks: HashMap<String, serde_json::Value>,
}

impl CarDecoded {
    /// Create an empty `CarDecoded` with no blocks.
    pub fn empty() -> Self {
        Self {
            blocks: HashMap::new(),
        }
    }
}

/// Decode a CAR v1 byte slice into its constituent blocks.
///
/// Returns a map of CID (hex-encoded) → decoded JSON value for each block.
/// Blocks that fail CBOR decoding are silently skipped.
pub fn decode_car(data: &[u8]) -> anyhow::Result<CarDecoded> {
    let mut cursor = data;

    // 1. Read the CAR header: varint length, then CBOR header map.
    let header_len = read_varint(&mut cursor)? as usize;
    if cursor.len() < header_len {
        anyhow::bail!("CAR header length {header_len} exceeds remaining data");
    }

    // Skip the header CBOR (we don't need roots for our purposes).
    cursor = &cursor[header_len..];

    // 2. Iterate blocks: each is varint(cid_len + data_len), then CID bytes, then block data.
    let mut blocks = HashMap::new();

    while !cursor.is_empty() {
        let block_section_len = match read_varint(&mut cursor) {
            Ok(len) => len as usize,
            Err(_) => break,
        };

        if block_section_len == 0 || cursor.len() < block_section_len {
            break;
        }

        let block_section = &cursor[..block_section_len];
        cursor = &cursor[block_section_len..];

        // Parse the CID from the block section to find where data starts.
        let mut block_cursor = block_section;
        let cid_hex = match read_cid(&mut block_cursor) {
            Ok(cid) => cid,
            Err(_) => continue,
        };

        // The remainder after reading the CID is the block data (CBOR).
        if block_cursor.is_empty() {
            continue;
        }

        // Attempt to decode the CBOR block into a serde_json::Value.
        match ciborium::from_reader::<ciborium::Value, _>(block_cursor) {
            Ok(cbor_val) => {
                let json_val = cbor_to_json(&cbor_val);
                blocks.insert(cid_hex, json_val);
            }
            Err(_) => {
                // Silently skip blocks that fail CBOR decoding.
            }
        }
    }

    Ok(CarDecoded { blocks })
}

/// Read an unsigned varint (LEB128) from a byte slice, advancing the cursor.
pub(crate) fn read_varint(reader: &mut &[u8]) -> anyhow::Result<u64> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;

    loop {
        if reader.is_empty() {
            anyhow::bail!("unexpected end of data reading varint");
        }

        let byte = reader[0];
        *reader = &reader[1..];

        value |= u64::from(byte & 0x7F) << shift;

        if byte & 0x80 == 0 {
            return Ok(value);
        }

        shift += 7;
        if shift >= 64 {
            anyhow::bail!("varint overflow");
        }
    }
}

/// Read a CID from a byte slice, advancing the cursor past it.
///
/// Returns the CID bytes as a hex string. Supports CIDv1 (multicodec prefix)
/// and CIDv0 (raw 34-byte SHA2-256 multihash starting with 0x12 0x20).
fn read_cid(reader: &mut &[u8]) -> anyhow::Result<String> {
    let start = *reader;

    if reader.is_empty() {
        anyhow::bail!("empty data reading CID");
    }

    // CIDv0: starts with 0x12 0x20 (SHA2-256 multihash, 32 bytes digest).
    // Total 34 bytes.
    if reader.len() >= 2 && reader[0] == 0x12 && reader[1] == 0x20 {
        if reader.len() < 34 {
            anyhow::bail!("truncated CIDv0");
        }
        let cid_bytes = &reader[..34];
        *reader = &reader[34..];
        return Ok(hex::encode(cid_bytes));
    }

    // CIDv1: varint(version) + varint(codec) + multihash.
    let version = read_varint(reader)?;
    if version != 1 {
        anyhow::bail!("unsupported CID version: {version}");
    }

    let _codec = read_varint(reader)?;

    // Multihash: varint(hash_fn) + varint(digest_size) + digest_bytes.
    let _hash_fn = read_varint(reader)?;
    let digest_size = read_varint(reader)? as usize;

    if reader.len() < digest_size {
        anyhow::bail!(
            "truncated CID multihash digest: need {digest_size}, have {}",
            reader.len()
        );
    }
    *reader = &reader[digest_size..];

    // The CID is everything from `start` to the current position.
    let cid_len = start.len() - reader.len();
    let cid_bytes = &start[..cid_len];
    Ok(hex::encode(cid_bytes))
}

/// Convert a `ciborium::Value` to a `serde_json::Value`.
///
/// CBOR types that have no direct JSON equivalent (bytes, tags) are
/// converted to string representations.
fn cbor_to_json(val: &ciborium::Value) -> serde_json::Value {
    match val {
        ciborium::Value::Null => serde_json::Value::Null,
        ciborium::Value::Bool(b) => serde_json::Value::Bool(*b),
        ciborium::Value::Integer(i) => {
            let n: i128 = (*i).into();
            if let Ok(v) = i64::try_from(n) {
                serde_json::Value::Number(v.into())
            } else if let Ok(v) = u64::try_from(n) {
                serde_json::Value::Number(v.into())
            } else {
                // Fallback for very large integers.
                serde_json::Value::String(n.to_string())
            }
        }
        ciborium::Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        ciborium::Value::Text(s) => serde_json::Value::String(s.clone()),
        ciborium::Value::Bytes(b) => {
            // AT Protocol uses CBOR bytes for CID links. Represent as an
            // object with `$bytes` key (matching common DAG-CBOR conventions).
            serde_json::json!({ "$bytes": hex::encode(b) })
        }
        ciborium::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(cbor_to_json).collect())
        }
        ciborium::Value::Map(entries) => {
            let mut map = serde_json::Map::new();
            for (k, v) in entries {
                let key = match k {
                    ciborium::Value::Text(s) => s.clone(),
                    other => format!("{other:?}"),
                };
                map.insert(key, cbor_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        ciborium::Value::Tag(tag, inner) => {
            // DAG-CBOR tag 42 is a CID link.
            if *tag == 42 {
                if let ciborium::Value::Bytes(b) = inner.as_ref() {
                    // CID link bytes — skip the leading 0x00 identity multibase prefix if present.
                    let cid_bytes = if b.first() == Some(&0x00) { &b[1..] } else { b };
                    return serde_json::json!({ "$link": hex::encode(cid_bytes) });
                }
            }
            // Generic tagged value fallback.
            serde_json::json!({
                "$tag": tag,
                "$value": cbor_to_json(inner),
            })
        }
        _ => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_varint_single_byte() {
        let data = [0x05u8];
        let mut cursor: &[u8] = &data;
        let val = read_varint(&mut cursor).unwrap();
        assert_eq!(val, 5);
        assert!(cursor.is_empty());
    }

    #[test]
    fn read_varint_two_bytes() {
        // 300 = 0b100101100 → LEB128: 0xAC 0x02
        let data = [0xACu8, 0x02];
        let mut cursor: &[u8] = &data;
        let val = read_varint(&mut cursor).unwrap();
        assert_eq!(val, 300);
        assert!(cursor.is_empty());
    }

    #[test]
    fn read_varint_zero() {
        let data = [0x00u8];
        let mut cursor: &[u8] = &data;
        let val = read_varint(&mut cursor).unwrap();
        assert_eq!(val, 0);
    }

    #[test]
    fn read_varint_empty_fails() {
        let data: &[u8] = &[];
        let mut cursor = data;
        assert!(read_varint(&mut cursor).is_err());
    }

    #[test]
    fn read_varint_max_single() {
        let data = [0x7Fu8]; // 127
        let mut cursor: &[u8] = &data;
        let val = read_varint(&mut cursor).unwrap();
        assert_eq!(val, 127);
    }

    #[test]
    fn read_varint_128() {
        // 128 → LEB128: 0x80 0x01
        let data = [0x80u8, 0x01];
        let mut cursor: &[u8] = &data;
        let val = read_varint(&mut cursor).unwrap();
        assert_eq!(val, 128);
    }

    #[test]
    fn cbor_to_json_null() {
        let val = ciborium::Value::Null;
        assert_eq!(cbor_to_json(&val), serde_json::Value::Null);
    }

    #[test]
    fn cbor_to_json_string() {
        let val = ciborium::Value::Text("hello".to_string());
        assert_eq!(cbor_to_json(&val), serde_json::json!("hello"));
    }

    #[test]
    fn cbor_to_json_map() {
        let val = ciborium::Value::Map(vec![(
            ciborium::Value::Text("key".to_string()),
            ciborium::Value::Integer(42.into()),
        )]);
        assert_eq!(cbor_to_json(&val), serde_json::json!({"key": 42}));
    }

    #[test]
    fn cbor_to_json_tag_42_cid_link() {
        // Tag 42 with bytes (with leading 0x00 multibase prefix).
        let mut cid_bytes = vec![0x00];
        cid_bytes.extend_from_slice(&[0x01, 0x71, 0x12, 0x20]);
        cid_bytes.extend_from_slice(&[0xAA; 32]);
        let val = ciborium::Value::Tag(42, Box::new(ciborium::Value::Bytes(cid_bytes)));
        let json = cbor_to_json(&val);
        assert!(json.get("$link").is_some());
    }

    #[test]
    fn empty_returns_no_blocks() {
        let decoded = CarDecoded::empty();
        assert!(decoded.blocks.is_empty());
    }

    #[test]
    fn decode_car_empty_blocks() {
        // Construct a minimal CAR v1: header only, no blocks.
        // Header CBOR: {"version": 1, "roots": []}
        let mut header_cbor = Vec::new();
        ciborium::into_writer(
            &ciborium::Value::Map(vec![
                (
                    ciborium::Value::Text("version".to_string()),
                    ciborium::Value::Integer(1.into()),
                ),
                (
                    ciborium::Value::Text("roots".to_string()),
                    ciborium::Value::Array(vec![]),
                ),
            ]),
            &mut header_cbor,
        )
        .unwrap();

        let mut data = Vec::new();
        // Write header length as varint.
        write_varint_to(&mut data, header_cbor.len() as u64);
        data.extend_from_slice(&header_cbor);

        let decoded = decode_car(&data).unwrap();
        assert!(decoded.blocks.is_empty());
    }

    /// Helper to write a varint for test construction.
    fn write_varint_to(buf: &mut Vec<u8>, mut val: u64) {
        loop {
            let mut byte = (val & 0x7F) as u8;
            val >>= 7;
            if val != 0 {
                byte |= 0x80;
            }
            buf.push(byte);
            if val == 0 {
                break;
            }
        }
    }

    // ── read_cid tests ──────────────────────────────────────────────

    #[test]
    fn read_cid_v0() {
        // CIDv0: starts with 0x12 0x20, followed by 32 bytes of digest. Total 34 bytes.
        let mut data = vec![0x12u8, 0x20];
        data.extend_from_slice(&[0xAA; 32]);
        let mut cursor: &[u8] = &data;
        let cid = read_cid(&mut cursor).unwrap();
        // Should consume all 34 bytes
        assert!(cursor.is_empty());
        // Hex should start with "1220"
        assert!(cid.starts_with("1220"));
        assert_eq!(cid.len(), 68); // 34 bytes * 2 hex chars
    }

    #[test]
    fn read_cid_v1() {
        let mut data = Vec::new();
        write_varint_to(&mut data, 1); // version
        write_varint_to(&mut data, 0x71); // codec (dag-cbor)
        write_varint_to(&mut data, 0x12); // hash fn (sha2-256)
        write_varint_to(&mut data, 32); // digest size
        data.extend_from_slice(&[0xBB; 32]); // digest
        let expected_len = data.len();
        let mut cursor: &[u8] = &data;
        let cid = read_cid(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(cid.len(), expected_len * 2); // hex encoding doubles length
    }

    #[test]
    fn read_cid_empty_fails() {
        let data: &[u8] = &[];
        let mut cursor = data;
        assert!(read_cid(&mut cursor).is_err());
    }

    #[test]
    fn read_cid_truncated_v0_fails() {
        // Only 10 bytes when CIDv0 needs 34
        let mut data = vec![0x12u8, 0x20];
        data.extend_from_slice(&[0xAA; 8]); // only 10 total, need 34
        let mut cursor: &[u8] = &data;
        assert!(read_cid(&mut cursor).is_err());
    }

    // ── decode_car with actual blocks ───────────────────────────────

    #[test]
    fn decode_car_with_one_block() {
        // Build header
        let mut header_cbor = Vec::new();
        ciborium::into_writer(
            &ciborium::Value::Map(vec![
                (
                    ciborium::Value::Text("version".into()),
                    ciborium::Value::Integer(1.into()),
                ),
                (
                    ciborium::Value::Text("roots".into()),
                    ciborium::Value::Array(vec![]),
                ),
            ]),
            &mut header_cbor,
        )
        .unwrap();

        // Build a CIDv1
        let mut cid_bytes = Vec::new();
        write_varint_to(&mut cid_bytes, 1); // version
        write_varint_to(&mut cid_bytes, 0x71); // codec
        write_varint_to(&mut cid_bytes, 0x12); // hash fn
        write_varint_to(&mut cid_bytes, 32); // digest size
        cid_bytes.extend_from_slice(&[0xCC; 32]); // digest

        // Build CBOR block data: {"hello": "world"}
        let mut block_data = Vec::new();
        ciborium::into_writer(
            &ciborium::Value::Map(vec![(
                ciborium::Value::Text("hello".into()),
                ciborium::Value::Text("world".into()),
            )]),
            &mut block_data,
        )
        .unwrap();

        // Block section = CID bytes + block data
        let mut block_section = Vec::new();
        block_section.extend_from_slice(&cid_bytes);
        block_section.extend_from_slice(&block_data);

        // Assemble full CAR
        let mut car_data = Vec::new();
        write_varint_to(&mut car_data, header_cbor.len() as u64);
        car_data.extend_from_slice(&header_cbor);
        write_varint_to(&mut car_data, block_section.len() as u64);
        car_data.extend_from_slice(&block_section);

        let decoded = decode_car(&car_data).unwrap();
        assert_eq!(decoded.blocks.len(), 1);
        let (_cid, value) = decoded.blocks.iter().next().unwrap();
        assert_eq!(value["hello"], "world");
    }

    // ── cbor_to_json missing branches ───────────────────────────────

    #[test]
    fn cbor_to_json_bool() {
        assert_eq!(
            cbor_to_json(&ciborium::Value::Bool(true)),
            serde_json::json!(true)
        );
        assert_eq!(
            cbor_to_json(&ciborium::Value::Bool(false)),
            serde_json::json!(false)
        );
    }

    #[test]
    fn cbor_to_json_integer() {
        assert_eq!(
            cbor_to_json(&ciborium::Value::Integer(42.into())),
            serde_json::json!(42)
        );
        assert_eq!(
            cbor_to_json(&ciborium::Value::Integer((-1).into())),
            serde_json::json!(-1)
        );
    }

    #[test]
    fn cbor_to_json_float() {
        let val = cbor_to_json(&ciborium::Value::Float(1.234));
        assert!(val.is_number());
    }

    #[test]
    fn cbor_to_json_bytes() {
        let val = cbor_to_json(&ciborium::Value::Bytes(vec![0xDE, 0xAD]));
        assert_eq!(val["$bytes"], "dead");
    }

    #[test]
    fn cbor_to_json_array() {
        let val = cbor_to_json(&ciborium::Value::Array(vec![
            ciborium::Value::Integer(1.into()),
            ciborium::Value::Text("two".into()),
        ]));
        assert_eq!(val, serde_json::json!([1, "two"]));
    }

    #[test]
    fn cbor_to_json_generic_tag() {
        // Non-42 tag should use $tag/$value fallback
        let val = cbor_to_json(&ciborium::Value::Tag(
            99,
            Box::new(ciborium::Value::Text("inner".into())),
        ));
        assert_eq!(val["$tag"], 99);
        assert_eq!(val["$value"], "inner");
    }

    #[test]
    fn cbor_to_json_float_nan() {
        // NaN can't be represented in JSON, should become null
        let val = cbor_to_json(&ciborium::Value::Float(f64::NAN));
        assert!(val.is_null());
    }

    // ── decode_car edge cases ───────────────────────────────────────

    #[test]
    fn decode_car_truncated_header_fails() {
        // varint says header is 100 bytes but only 5 available
        let mut data = Vec::new();
        write_varint_to(&mut data, 100);
        data.extend_from_slice(&[0; 5]);
        assert!(decode_car(&data).is_err());
    }

    #[test]
    fn decode_car_zero_length_block_skipped() {
        // Header + a block with 0 length should be skipped gracefully
        let mut header_cbor = Vec::new();
        ciborium::into_writer(
            &ciborium::Value::Map(vec![
                (
                    ciborium::Value::Text("version".into()),
                    ciborium::Value::Integer(1.into()),
                ),
                (
                    ciborium::Value::Text("roots".into()),
                    ciborium::Value::Array(vec![]),
                ),
            ]),
            &mut header_cbor,
        )
        .unwrap();

        let mut data = Vec::new();
        write_varint_to(&mut data, header_cbor.len() as u64);
        data.extend_from_slice(&header_cbor);
        write_varint_to(&mut data, 0); // zero-length block

        let decoded = decode_car(&data).unwrap();
        assert!(decoded.blocks.is_empty());
    }
}
