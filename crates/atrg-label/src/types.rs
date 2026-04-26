//! Label types matching the `com.atproto.label.defs` schema.

use serde::{Deserialize, Serialize};

/// Configuration for the labeler service.
#[derive(Debug, Clone, Deserialize)]
pub struct LabelerConfig {
    /// DID of the labeler service.
    pub did: String,
    /// Path to the signing key file (PEM format).
    pub signing_key_path: Option<String>,
    /// Inline signing key (base64-encoded, for env var injection).
    pub signing_key_base64: Option<String>,
}

/// A label as defined by `com.atproto.label.defs#label`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    /// Version of the label format (currently 1).
    #[serde(default = "default_version")]
    pub ver: i32,
    /// DID of the labeler that created this label.
    pub src: String,
    /// AT-URI of the subject being labeled.
    pub uri: String,
    /// CID of the subject (optional, for specific record versions).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    /// The label value (e.g. "porn", "spam", "misleading").
    pub val: String,
    /// Whether this is a negation (removal) of a previous label.
    #[serde(default)]
    pub neg: bool,
    /// Timestamp when the label was created (ISO 8601).
    pub cts: String,
    /// Expiration timestamp (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<String>,
}

fn default_version() -> i32 {
    1
}

/// A label with its cryptographic signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedLabel {
    /// The label data.
    #[serde(flatten)]
    pub label: Label,
    /// Base64-encoded signature over the CBOR-serialized label.
    pub sig: String,
}

/// Enumeration of well-known label values.
///
/// This is not exhaustive — labelers can define custom values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LabelValue {
    /// Adult content — pornography.
    Porn,
    /// Adult content — sexual.
    Sexual,
    /// Adult content — nudity.
    Nudity,
    /// Graphic media.
    GraphicMedia,
    /// Spam content.
    Spam,
    /// Impersonation.
    Impersonation,
    /// Custom label value.
    Custom(String),
}

impl LabelValue {
    /// Convert to the string representation used in the protocol.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Porn => "porn",
            Self::Sexual => "sexual",
            Self::Nudity => "nudity",
            Self::GraphicMedia => "graphic-media",
            Self::Spam => "spam",
            Self::Impersonation => "impersonation",
            Self::Custom(s) => s.as_str(),
        }
    }
}

impl std::fmt::Display for LabelValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_value_as_str() {
        assert_eq!(LabelValue::Porn.as_str(), "porn");
        assert_eq!(LabelValue::Sexual.as_str(), "sexual");
        assert_eq!(LabelValue::Nudity.as_str(), "nudity");
        assert_eq!(LabelValue::GraphicMedia.as_str(), "graphic-media");
        assert_eq!(LabelValue::Spam.as_str(), "spam");
        assert_eq!(LabelValue::Impersonation.as_str(), "impersonation");
        assert_eq!(
            LabelValue::Custom("custom-val".into()).as_str(),
            "custom-val"
        );
    }

    #[test]
    fn label_value_display() {
        assert_eq!(format!("{}", LabelValue::Porn), "porn");
        assert_eq!(format!("{}", LabelValue::Custom("test".into())), "test");
    }

    #[test]
    fn label_serde_roundtrip() {
        let label = Label {
            ver: 1,
            src: "did:plc:labeler".to_string(),
            uri: "at://did:plc:user/app.bsky.feed.post/abc".to_string(),
            cid: Some("bafyreib".to_string()),
            val: "spam".to_string(),
            neg: false,
            cts: "2024-01-01T00:00:00Z".to_string(),
            exp: None,
        };

        let json = serde_json::to_string(&label).unwrap();
        let parsed: Label = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.ver, 1);
        assert_eq!(parsed.src, "did:plc:labeler");
        assert_eq!(parsed.uri, "at://did:plc:user/app.bsky.feed.post/abc");
        assert_eq!(parsed.cid.as_deref(), Some("bafyreib"));
        assert_eq!(parsed.val, "spam");
        assert!(!parsed.neg);
        assert_eq!(parsed.cts, "2024-01-01T00:00:00Z");
        assert!(parsed.exp.is_none());
    }

    #[test]
    fn label_serde_optional_fields_omitted() {
        let label = Label {
            ver: 1,
            src: "did:plc:labeler".to_string(),
            uri: "at://did:plc:user/app.bsky.feed.post/abc".to_string(),
            cid: None,
            val: "spam".to_string(),
            neg: false,
            cts: "2024-01-01T00:00:00Z".to_string(),
            exp: None,
        };

        let json = serde_json::to_string(&label).unwrap();
        assert!(!json.contains("cid"));
        assert!(!json.contains("exp"));
    }

    #[test]
    fn signed_label_flattens() {
        let signed = SignedLabel {
            label: Label {
                ver: 1,
                src: "did:plc:labeler".to_string(),
                uri: "at://did:plc:user/post/1".to_string(),
                cid: None,
                val: "porn".to_string(),
                neg: false,
                cts: "2024-01-01T00:00:00Z".to_string(),
                exp: None,
            },
            sig: "c2lnbmF0dXJl".to_string(),
        };

        let json = serde_json::to_string(&signed).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["src"], "did:plc:labeler");
        assert_eq!(v["sig"], "c2lnbmF0dXJl");
    }
}
