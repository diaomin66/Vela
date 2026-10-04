use super::{files, parsing::Parsed};
use serde::{
    de::{IgnoredAny, MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer,
};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    fs::{self, File},
    io::{self, BufRead, BufReader, Read},
    path::Path,
    rc::Rc,
};

pub(super) struct Streamed {
    pub parsed: Parsed,
    pub bytes: u64,
    pub fingerprint: String,
    pub prefix_bytes: u64,
    pub prefix_fingerprint: String,
    pub modified_at: Option<String>,
}

#[derive(Default)]
struct Digests {
    full: Sha256,
    prefix: Sha256,
    bytes: u64,
    prefix_bytes: u64,
}

struct HashedReader {
    file: File,
    state: Rc<RefCell<Digests>>,
}

impl Read for HashedReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let length = self.file.read(output)?;
        let mut state = self.state.borrow_mut();
        if let Some(position) = output[..length].iter().rposition(|byte| *byte == b'\n') {
            let mut prefix = state.full.clone();
            prefix.update(&output[..=position]);
            state.prefix = prefix;
            state.prefix_bytes = state.bytes + position as u64 + 1;
        }
        state.full.update(&output[..length]);
        state.bytes += length as u64;
        Ok(length)
    }
}

struct JsonLine<'a, R> {
    reader: &'a mut R,
    ended: bool,
    non_whitespace: bool,
}

impl<R: BufRead> Read for JsonLine<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.ended || output.is_empty() {
            return Ok(0);
        }
        let available = self.reader.fill_buf()?;
        let available = &available[..available.len().min(output.len())];
        let length = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|position| position + 1)
            .unwrap_or(available.len());
        output[..length].copy_from_slice(&available[..length]);
        self.ended = output[..length].last() == Some(&b'\n');
        self.non_whitespace |= output[..length]
            .iter()
            .any(|byte| !byte.is_ascii_whitespace());
        self.reader.consume(length);
        Ok(length)
    }
}

#[derive(Deserialize)]
struct Record {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    payload: Option<Metadata>,
}

#[derive(Default, Deserialize)]
struct Metadata {
    #[serde(default, deserialize_with = "optional_string")]
    id: Option<String>,
    #[serde(default, deserialize_with = "optional_string")]
    thread_id: Option<String>,
    #[serde(default, deserialize_with = "optional_string")]
    session_id: Option<String>,
    #[serde(default, deserialize_with = "optional_string")]
    timestamp: Option<String>,
    #[serde(default, deserialize_with = "optional_string")]
    cwd: Option<String>,
    #[serde(default, deserialize_with = "optional_string")]
    model_provider: Option<String>,
    source: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "optional_string")]
    history_mode: Option<String>,
    history_base: Option<serde_json::Value>,
}

// Other event payloads may reuse metadata field names with different value
// types. Ignore those values without materializing their nested content.
fn optional_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    struct StringOrIgnored;
    impl<'de> Visitor<'de> for StringOrIgnored {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("any JSON value")
        }

        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
            Ok(Some(value.to_owned()))
        }

        fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
            Ok(Some(value))
        }

        fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
            while sequence.next_element::<IgnoredAny>()?.is_some() {}
            Ok(None)
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            Ok(None)
        }
    }
    deserializer.deserialize_any(StringOrIgnored)
}

pub(super) fn scan(root: &Path, path: &Path) -> Result<Streamed, files::ReadFailure> {
    files::validate_path(root, path)?;
    let before = fs::metadata(path).map_err(|_| files::ReadFailure::Unreadable)?;
    let file = File::open(path).map_err(|_| files::ReadFailure::Unreadable)?;
    let held = file
        .try_clone()
        .map_err(|_| files::ReadFailure::Unreadable)?;
    let opened = held
        .metadata()
        .map_err(|_| files::ReadFailure::Unreadable)?;
    if !files::same_version(&before, &opened) {
        return Err(files::ReadFailure::Changed);
    }
    let digests = Rc::new(RefCell::new(Digests::default()));
    let raw = HashedReader {
        file,
        state: Rc::clone(&digests),
    };
    let compressed = path.extension().is_some_and(|extension| extension == "zst");
    let input: Box<dyn Read> = if compressed {
        Box::new(zstd::stream::read::Decoder::new(raw).map_err(|_| files::ReadFailure::Unreadable)?)
    } else {
        Box::new(raw)
    };
    let mut input = BufReader::with_capacity(64 * 1024, input);
    let parsed = parse(&mut input, path)?;
    let after = fs::metadata(path).map_err(|_| files::ReadFailure::Changed)?;
    let held_after = held.metadata().map_err(|_| files::ReadFailure::Changed)?;
    let digests = digests.borrow();
    if !files::same_version(&before, &after)
        || !files::same_version(&before, &held_after)
        || digests.bytes != before.len()
    {
        return Err(files::ReadFailure::Changed);
    }
    let fingerprint = format!("{:x}", digests.full.clone().finalize());
    Ok(Streamed {
        parsed,
        bytes: digests.bytes,
        prefix_bytes: if compressed {
            digests.bytes
        } else {
            digests.prefix_bytes
        },
        prefix_fingerprint: if compressed {
            fingerprint.clone()
        } else {
            format!("{:x}", digests.prefix.clone().finalize())
        },
        fingerprint,
        modified_at: before
            .modified()
            .ok()
            .map(chrono::DateTime::<chrono::Utc>::from)
            .map(|time| time.to_rfc3339()),
    })
}

fn parse(reader: &mut impl BufRead, path: &Path) -> Result<Parsed, files::ReadFailure> {
    let mut parsed = Parsed::default();
    let mut own_metadata = false;
    let mut corrupt = false;
    let mut unsupported = false;
    let mut partial = false;
    loop {
        if reader
            .fill_buf()
            .map_err(|_| files::ReadFailure::Unreadable)?
            .is_empty()
        {
            break;
        }
        let mut line = JsonLine {
            reader,
            ended: false,
            non_whitespace: false,
        };
        let record = serde_json::from_reader::<_, Record>(&mut line);
        io::copy(&mut line, &mut io::sink()).map_err(|_| files::ReadFailure::Unreadable)?;
        if !line.non_whitespace {
            continue;
        }
        if !line.ended {
            partial = true;
            continue;
        }
        parsed.lines += 1;
        let record = match record {
            Ok(record) => record,
            Err(error) if error.is_io() => return Err(files::ReadFailure::Unreadable),
            Err(_) => {
                corrupt = true;
                continue;
            }
        };
        let Some(payload) = record.payload else {
            unsupported = true;
            continue;
        };
        match record.kind.as_deref() {
            Some("session_meta") if !own_metadata => {
                own_metadata = true;
                parsed.thread_id = payload
                    .id
                    .or(payload.thread_id)
                    .or(payload.session_id)
                    .filter(|id| uuid::Uuid::parse_str(id).is_ok());
                parsed.cwd = payload.cwd;
                parsed.provider = payload.model_provider;
                parsed.created_at = payload.timestamp.or(record.timestamp);
                parsed.source_kind = payload.source.and_then(|source| {
                    source.as_str().map(str::to_owned).or_else(|| {
                        source
                            .as_object()
                            .and_then(|source| source.keys().next().cloned())
                    })
                });
                match super::parsing::history_base(payload.history_base.as_ref()) {
                    Ok(base) => parsed.history_base = base,
                    Err(()) => unsupported = true,
                }
                unsupported |= payload
                    .history_mode
                    .as_deref()
                    .is_some_and(|mode| !matches!(mode, "legacy" | "paginated"));
            }
            Some("turn_context") => {
                parsed.cwd = payload.cwd.or(parsed.cwd);
            }
            None => {
                unsupported = true;
            }
            _ => {}
        }
    }
    let mismatch = super::parsing::filename_id(path)
        .zip(parsed.thread_id.as_ref())
        .is_some_and(|(filename, actual)| &filename != actual);
    parsed.integrity = if unsupported || !own_metadata || parsed.thread_id.is_none() || mismatch {
        "unrecognized"
    } else if corrupt {
        "corrupt"
    } else if partial {
        "partial"
    } else {
        "valid"
    }
    .into();
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn streaming_hashes_complete_prefix_and_skips_large_message_payloads() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rollout.jsonl");
        let metadata = b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"0199a5d0-9ac1-79d0-8be1-223344556677\"}}\n";
        let mut contents = metadata.to_vec();
        contents.extend_from_slice(b"{\"type\":\"response_item\",\"payload\":{\"content\":[{\"type\":\"input_image\",\"image_url\":\"");
        contents.extend(std::iter::repeat_n(b'x', 3 * 1024 * 1024));
        contents.extend_from_slice(b"\"}]}}\n");
        let prefix = contents.len();
        contents.extend_from_slice(b"{partial");
        fs::write(&path, &contents).unwrap();
        let streamed = scan(directory.path(), &path).ok().unwrap();
        assert_eq!(streamed.parsed.integrity, "partial");
        assert_eq!(streamed.parsed.lines, 2);
        assert_eq!(streamed.prefix_bytes as usize, prefix);
        assert_eq!(streamed.fingerprint, super::super::digest(&contents));
        assert_eq!(
            streamed.prefix_fingerprint,
            super::super::digest(&contents[..prefix])
        );
        assert!(streamed.parsed.preview.is_empty());
    }

    #[test]
    fn compressed_stream_uses_original_bytes_hash_and_rejects_damaged_frames() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rollout.jsonl.zst");
        let contents = b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"0199a5d0-9ac1-79d0-8be1-223344556677\"}}\n";
        let mut encoder = zstd::stream::Encoder::new(Vec::new(), 1).unwrap();
        encoder.write_all(contents).unwrap();
        let encoded = encoder.finish().unwrap();
        fs::write(&path, &encoded).unwrap();
        let streamed = scan(directory.path(), &path).ok().unwrap();
        assert_eq!(streamed.parsed.integrity, "valid");
        assert_eq!(streamed.fingerprint, super::super::digest(&encoded));
        assert_eq!(streamed.bytes, encoded.len() as u64);
        fs::write(&path, &encoded[..encoded.len() - 2]).unwrap();
        assert!(scan(directory.path(), &path).is_err());
    }

    #[test]
    fn non_metadata_fields_with_other_json_types_do_not_corrupt_the_stream() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rollout.jsonl");
        let contents = concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"0199a5d0-9ac1-79d0-8be1-223344556677\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"id\":42,\"cwd\":[\"unused\"],\"history_mode\":{\"value\":true}}}\n"
        );
        fs::write(&path, contents).unwrap();
        let streamed = scan(directory.path(), &path).ok().unwrap();
        assert_eq!(streamed.parsed.integrity, "valid");
        assert_eq!(streamed.parsed.lines, 2);
    }
}
