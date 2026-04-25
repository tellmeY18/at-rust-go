//! AT Protocol lexicon JSON parsing.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A parsed AT Protocol lexicon document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LexiconDoc {
    /// The lexicon version (should be 1).
    pub lexicon: u32,
    /// The NSID (e.g. "com.example.getPosts").
    pub id: String,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
    /// The definitions in this lexicon.
    #[serde(default)]
    pub defs: HashMap<String, LexiconDef>,
}

/// A single definition within a lexicon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LexiconDef {
    /// A record type.
    #[serde(rename = "record")]
    Record {
        /// Description.
        #[serde(default)]
        description: Option<String>,
        /// Record key type.
        #[serde(default)]
        key: Option<String>,
        /// The record schema.
        #[serde(default)]
        record: Option<LexiconObject>,
    },
    /// An object type.
    #[serde(rename = "object")]
    Object(LexiconObject),
    /// A query (read) procedure.
    #[serde(rename = "query")]
    Query {
        /// Description.
        #[serde(default)]
        description: Option<String>,
        /// Query parameters.
        #[serde(default)]
        parameters: Option<LexiconObject>,
        /// Output schema.
        #[serde(default)]
        output: Option<LexiconOutput>,
    },
    /// A mutation procedure.
    #[serde(rename = "procedure")]
    Procedure {
        /// Description.
        #[serde(default)]
        description: Option<String>,
        /// Input body schema.
        #[serde(default)]
        input: Option<LexiconBody>,
        /// Output schema.
        #[serde(default)]
        output: Option<LexiconOutput>,
    },
    /// A string type.
    #[serde(rename = "string")]
    String {
        /// Description.
        #[serde(default)]
        description: Option<String>,
        /// Known values.
        #[serde(default)]
        known_values: Option<Vec<String>>,
    },
    /// A token type.
    #[serde(rename = "token")]
    Token {
        /// Description.
        #[serde(default)]
        description: Option<String>,
    },
    /// Catch-all for types we don't generate code for yet.
    #[serde(other)]
    Unknown,
}

/// An object schema (used in records, params, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LexiconObject {
    /// Description.
    #[serde(default)]
    pub description: Option<String>,
    /// Required field names.
    #[serde(default)]
    pub required: Vec<String>,
    /// Property definitions.
    #[serde(default)]
    pub properties: HashMap<String, LexiconProperty>,
}

/// A single property in an object schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LexiconProperty {
    /// The type of this property.
    #[serde(rename = "type")]
    pub prop_type: String,
    /// Description.
    #[serde(default)]
    pub description: Option<String>,
    /// Format hint (e.g. "datetime", "uri").
    #[serde(default)]
    pub format: Option<String>,
    /// Maximum length.
    #[serde(default)]
    pub max_length: Option<u64>,
    /// Minimum value.
    #[serde(default)]
    pub minimum: Option<i64>,
    /// Maximum value.
    #[serde(default)]
    pub maximum: Option<i64>,
    /// Default value.
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    /// Items type (for arrays).
    #[serde(default)]
    pub items: Option<Box<LexiconProperty>>,
    /// Reference to another type.
    #[serde(rename = "ref", default)]
    pub ref_: Option<String>,
}

/// Output schema for queries/procedures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LexiconOutput {
    /// Encoding (usually "application/json").
    #[serde(default)]
    pub encoding: Option<String>,
    /// The schema.
    #[serde(default)]
    pub schema: Option<LexiconObject>,
}

/// Input body for procedures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LexiconBody {
    /// Encoding.
    #[serde(default)]
    pub encoding: Option<String>,
    /// The schema.
    #[serde(default)]
    pub schema: Option<LexiconObject>,
}

/// Load a lexicon document from a JSON string.
pub fn parse_lexicon(json: &str) -> anyhow::Result<LexiconDoc> {
    let doc: LexiconDoc = serde_json::from_str(json)?;
    if doc.lexicon != 1 {
        anyhow::bail!("unsupported lexicon version: {} (expected 1)", doc.lexicon);
    }
    if doc.id.is_empty() {
        anyhow::bail!("lexicon id must not be empty");
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_lexicon() {
        let json = r#"{
            "lexicon": 1,
            "id": "com.example.ping",
            "defs": {
                "main": {
                    "type": "query",
                    "description": "A simple ping",
                    "output": {
                        "encoding": "application/json",
                        "schema": {
                            "type": "object",
                            "properties": {
                                "pong": { "type": "boolean" }
                            }
                        }
                    }
                }
            }
        }"#;
        let doc = parse_lexicon(json).unwrap();
        assert_eq!(doc.id, "com.example.ping");
        assert!(doc.defs.contains_key("main"));
    }

    #[test]
    fn parse_record_lexicon() {
        let json = r#"{
            "lexicon": 1,
            "id": "com.example.post",
            "defs": {
                "main": {
                    "type": "record",
                    "description": "A post record",
                    "key": "tid",
                    "record": {
                        "type": "object",
                        "required": ["text", "createdAt"],
                        "properties": {
                            "text": { "type": "string", "max_length": 3000 },
                            "createdAt": { "type": "string", "format": "datetime" }
                        }
                    }
                }
            }
        }"#;
        let doc = parse_lexicon(json).unwrap();
        assert_eq!(doc.id, "com.example.post");
    }

    #[test]
    fn invalid_version_rejected() {
        let json = r#"{"lexicon": 2, "id": "com.example.test", "defs": {}}"#;
        assert!(parse_lexicon(json).is_err());
    }

    #[test]
    fn empty_id_rejected() {
        let json = r#"{"lexicon": 1, "id": "", "defs": {}}"#;
        assert!(parse_lexicon(json).is_err());
    }

    #[test]
    fn malformed_json_gives_error() {
        assert!(parse_lexicon("not json").is_err());
    }
}
