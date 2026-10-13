//! Lossless editor buffer. Text history/cursor/IME belong to the UI editor;
//! this layer owns validation, the last valid preview and save snapshots.

use crate::Diagnostic;
use crate::resume::{MAX_RESUME_BYTES, ResumeDocument, parse_resume, revision};

#[derive(Debug, Clone)]
pub struct SaveSnapshot {
    pub source: Vec<u8>,
    pub if_match: String,
}

#[derive(Debug, Clone)]
pub struct ResumeBuffer {
    source: String,
    baseline_revision: String,
    source_revision: String,
    last_valid: Option<ResumeDocument>,
    issues: Vec<Diagnostic>,
}

impl ResumeBuffer {
    /// Invalid JSON is still editable. Invalid UTF-8 is rejected rather than
    /// silently replacing bytes, which would corrupt the original document.
    pub fn new(source: Vec<u8>) -> Result<Self, Diagnostic> {
        let baseline_revision = revision(&source);
        let source = String::from_utf8(source).map_err(|_| Diagnostic {
            code: "RESUME_ENCODING",
            path: String::new(),
            message: "文件不是 UTF-8；请先转换编码，原文件未修改".into(),
        })?;
        let mut buffer = Self {
            source,
            source_revision: baseline_revision.clone(),
            baseline_revision,
            last_valid: None,
            issues: Vec::new(),
        };
        buffer.validate();
        Ok(buffer)
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn is_dirty(&self) -> bool {
        self.source_revision != self.baseline_revision
    }

    pub fn issues(&self) -> &[Diagnostic] {
        &self.issues
    }

    pub fn last_valid(&self) -> Option<&ResumeDocument> {
        self.last_valid.as_ref()
    }

    /// Returns false for a redundant UI change event.
    pub fn set_source(&mut self, source: String) -> bool {
        if self.source == source {
            return false;
        }
        self.source_revision = revision(source.as_bytes());
        self.source = source;
        self.validate();
        true
    }

    fn validate(&mut self) {
        match parse_resume(self.source.as_bytes()) {
            Ok(document) => {
                self.last_valid = Some(document);
                self.issues.clear();
            }
            Err(issues) => self.issues = issues,
        }
    }

    /// Format syntax-valid JSON even when its content fails the resume schema,
    /// so formatting can help repair it. Never auto-coerce or delete fields.
    /// The caller inserts this as one undoable editor transaction.
    pub fn formatted(&self) -> Result<String, Diagnostic> {
        if self.source.len() > MAX_RESUME_BYTES {
            return Err(Diagnostic {
                code: "RESUME_TOO_LARGE",
                path: String::new(),
                message: "简历文件超过 16 MiB 上限，未格式化".into(),
            });
        }
        let value: serde_json::Value =
            serde_json::from_str(&self.source).map_err(|error| Diagnostic {
                code: "INVALID_JSON",
                path: String::new(),
                message: format!("无法格式化：{error}"),
            })?;
        let mut text = serde_json::to_string_pretty(&value).expect("JSON value is serializable");
        text.push('\n');
        Ok(text)
    }

    pub fn save_snapshot(&self) -> Result<SaveSnapshot, &[Diagnostic]> {
        if !self.issues.is_empty() {
            return Err(&self.issues);
        }
        Ok(SaveSnapshot {
            source: self.source.as_bytes().to_vec(),
            if_match: self.baseline_revision.clone(),
        })
    }

    /// Acknowledge the snapshot actually committed, NOT whatever the user may
    /// have typed during the write. Thus newer input remains dirty.
    pub fn acknowledge_save(&mut self, snapshot: &SaveSnapshot) -> bool {
        if snapshot.if_match != self.baseline_revision {
            return false;
        }
        self.baseline_revision = revision(&snapshot.source);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::replace_resume;

    #[test]
    fn untouched_buffer_preserves_every_byte_and_extensions() {
        let source = "{\r\n \"basics\": {\"name\": \"李明😀\"}, \"extra\": [1, true]\r\n}\r\n";
        let buffer = ResumeBuffer::new(source.as_bytes().to_vec()).unwrap();
        assert!(!buffer.is_dirty());
        assert_eq!(buffer.source(), source);
        assert_eq!(buffer.save_snapshot().unwrap().source, source.as_bytes());
        assert_eq!(buffer.last_valid().unwrap().value["extra"][1], true);
    }

    #[test]
    fn invalid_edits_preserve_text_and_last_valid_preview() {
        let mut buffer = ResumeBuffer::new(br#"{"basics":{"name":"A"}}"#.to_vec()).unwrap();
        buffer.set_source("{\"basics\": ".into());
        assert_eq!(buffer.source(), "{\"basics\": ");
        assert!(buffer.is_dirty());
        assert_eq!(buffer.issues()[0].code, "INVALID_JSON");
        assert_eq!(buffer.last_valid().unwrap().value["basics"]["name"], "A");
        assert!(buffer.save_snapshot().is_err());
        buffer.set_source(r#"{"basics":{"name":7}}"#.into());
        assert_eq!(buffer.issues()[0].path, "/basics/name");
        assert_eq!(buffer.last_valid().unwrap().value["basics"]["name"], "A");
        buffer.set_source(r#"{"basics":{"name":"B"}}"#.into());
        assert!(buffer.issues().is_empty());
        assert_eq!(buffer.last_valid().unwrap().value["basics"]["name"], "B");
    }

    #[test]
    fn formatting_keeps_order_and_unknown_fields_and_is_idempotent() {
        let mut buffer =
            ResumeBuffer::new(br#"{"extra":{"html":"<br>"},"basics":{"name":"A"}}"#.to_vec())
                .unwrap();
        let formatted = buffer.formatted().unwrap();
        assert!(formatted.find("extra").unwrap() < formatted.find("basics").unwrap());
        assert!(formatted.contains("<br>")); // Preserve user content; never inject it.
        assert!(formatted.ends_with('\n'));
        buffer.set_source(formatted.clone());
        assert_eq!(buffer.formatted().unwrap(), formatted);
        assert!(!buffer.set_source(formatted));
    }

    #[test]
    fn formatting_does_not_coerce_schema_errors_or_damage_broken_json() {
        let mut buffer = ResumeBuffer::new(br#"{"work":[{"name":42}]}"#.to_vec()).unwrap();
        assert!(buffer.last_valid().is_none());
        buffer.set_source(buffer.formatted().unwrap());
        assert_eq!(buffer.issues()[0].path, "/work/0/name");
        assert!(buffer.save_snapshot().is_err());
        buffer.set_source("{broken".into());
        assert!(buffer.formatted().is_err());
        assert_eq!(buffer.source(), "{broken");
    }

    #[test]
    fn undo_back_to_original_bytes_clears_dirty_flag() {
        let original = r#"{"basics":{}}"#;
        let mut buffer = ResumeBuffer::new(original.as_bytes().to_vec()).unwrap();
        buffer.set_source(buffer.formatted().unwrap());
        assert!(buffer.is_dirty());
        buffer.set_source(original.into());
        assert!(!buffer.is_dirty());
    }

    #[test]
    fn input_during_save_remains_dirty_and_uses_committed_revision_next_time() {
        let mut buffer = ResumeBuffer::new(b"{}".to_vec()).unwrap();
        buffer.set_source(r#"{"basics":{"name":"A"}}"#.into());
        let saved = buffer.save_snapshot().unwrap();
        buffer.set_source(r#"{"basics":{"name":"B"}}"#.into());
        assert!(buffer.acknowledge_save(&saved));
        assert!(buffer.is_dirty());
        assert_eq!(
            buffer.save_snapshot().unwrap().if_match,
            revision(&saved.source)
        );
        assert!(!buffer.acknowledge_save(&saved)); // Reject stale completion.
    }

    #[test]
    fn repaired_invalid_file_can_be_saved_without_metadata_or_reformatting() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("简历.json");
        std::fs::write(&path, b"{bad").unwrap();
        let mut buffer = ResumeBuffer::new(b"{bad".to_vec()).unwrap();
        let next = "{ \"basics\": {\"name\": \"修复\"}, \"custom\": null }\r\n";
        buffer.set_source(next.into());
        let snapshot = buffer.save_snapshot().unwrap();
        let committed = replace_resume(&path, &snapshot.source, &snapshot.if_match).unwrap();
        assert_eq!(committed, revision(next.as_bytes()));
        buffer.acknowledge_save(&snapshot);
        assert!(!buffer.is_dirty());
        assert_eq!(std::fs::read(&path).unwrap(), next.as_bytes());
    }

    #[test]
    fn rejects_invalid_encoding_without_lossy_replacement() {
        assert_eq!(
            ResumeBuffer::new(vec![0xff]).unwrap_err().code,
            "RESUME_ENCODING"
        );
    }

    #[test]
    fn oversized_input_stays_in_buffer_but_cannot_be_formatted_or_saved() {
        let mut buffer = ResumeBuffer::new(b"{}".to_vec()).unwrap();
        buffer.set_source(" ".repeat(MAX_RESUME_BYTES + 1));
        assert_eq!(buffer.issues()[0].code, "RESUME_TOO_LARGE");
        assert_eq!(buffer.formatted().unwrap_err().code, "RESUME_TOO_LARGE");
        assert!(buffer.save_snapshot().is_err());
        assert_eq!(buffer.last_valid().unwrap().source, b"{}");
    }

    #[test]
    fn formatting_retains_precision_of_unknown_numeric_fields() {
        let source = r#"{"basics":{"name":"A"},"extra":{"big":184467440737095516170123,"decimal":0.12345678901234567890123456789,"exponent":1e400}}"#;
        let buffer = ResumeBuffer::new(source.as_bytes().to_vec()).unwrap();
        assert!(buffer.issues().is_empty());
        let text = buffer.formatted().unwrap();
        assert!(text.contains("184467440737095516170123"));
        assert!(text.contains("0.12345678901234567890123456789"));
        assert_eq!(
            parse_resume(text.as_bytes()).unwrap().value["extra"],
            buffer.last_valid().unwrap().value["extra"]
        );
    }
}
