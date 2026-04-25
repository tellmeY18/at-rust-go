//! Mock AT Protocol client for testing.

use std::collections::HashMap;
use std::sync::Mutex;

/// A recorded call to the mock client.
#[derive(Debug, Clone)]
pub struct RecordedCall {
    /// The method name (e.g. "get_record", "put_record").
    pub method: String,
    /// The path or key.
    pub path: String,
    /// The request body (if any).
    pub body: Option<serde_json::Value>,
}

/// Mock AT Protocol client that records calls and returns scripted responses.
///
/// Use this in tests instead of making real network calls.
pub struct MockAtprotoClient {
    calls: Mutex<Vec<RecordedCall>>,
    responses: Mutex<HashMap<String, serde_json::Value>>,
}

impl MockAtprotoClient {
    /// Create a new empty mock client.
    pub fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            responses: Mutex::new(HashMap::new()),
        }
    }

    /// Script a response for a given method+path combination.
    pub fn when(&self, method: &str, path: &str) -> &Self {
        // Store the key for later; the response is set via `returns`
        self.responses
            .lock()
            .unwrap()
            .insert(format!("{}:{}", method, path), serde_json::json!(null));
        self
    }

    /// Set the response for the most recently configured `when`.
    pub fn returns(&self, method: &str, path: &str, response: serde_json::Value) {
        self.responses
            .lock()
            .unwrap()
            .insert(format!("{}:{}", method, path), response);
    }

    /// Simulate a get_record call.
    pub fn get_record(&self, collection: &str, rkey: &str) -> anyhow::Result<serde_json::Value> {
        let path = format!("{}/{}", collection, rkey);
        self.record_call("get_record", &path, None);
        self.get_response("get_record", &path)
    }

    /// Simulate a put_record call.
    pub fn put_record(
        &self,
        collection: &str,
        record: &serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        self.record_call("put_record", collection, Some(record.clone()));
        self.get_response("put_record", collection)
    }

    /// Simulate a list_records call.
    pub fn list_records(&self, collection: &str) -> anyhow::Result<serde_json::Value> {
        self.record_call("list_records", collection, None);
        self.get_response("list_records", collection)
    }

    /// Simulate a delete_record call.
    pub fn delete_record(&self, collection: &str, rkey: &str) -> anyhow::Result<()> {
        let path = format!("{}/{}", collection, rkey);
        self.record_call("delete_record", &path, None);
        Ok(())
    }

    /// Get all recorded calls.
    pub fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Assert that a specific method+path was called exactly `n` times.
    pub fn assert_called(&self, method: &str, path: &str, n: usize) {
        let calls = self.calls.lock().unwrap();
        let count = calls
            .iter()
            .filter(|c| c.method == method && c.path == path)
            .count();
        assert_eq!(
            count, n,
            "expected {} calls to {}:{}, got {}",
            n, method, path, count
        );
    }

    /// Assert that a method was called at least once (any path).
    pub fn assert_called_any(&self, method: &str) {
        let calls = self.calls.lock().unwrap();
        let count = calls.iter().filter(|c| c.method == method).count();
        assert!(count > 0, "expected at least one call to {}, got 0", method);
    }

    fn record_call(&self, method: &str, path: &str, body: Option<serde_json::Value>) {
        self.calls.lock().unwrap().push(RecordedCall {
            method: method.to_string(),
            path: path.to_string(),
            body,
        });
    }

    fn get_response(&self, method: &str, path: &str) -> anyhow::Result<serde_json::Value> {
        let key = format!("{}:{}", method, path);
        let responses = self.responses.lock().unwrap();
        match responses.get(&key) {
            Some(v) => Ok(v.clone()),
            None => Ok(serde_json::json!({})),
        }
    }
}

impl Default for MockAtprotoClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_records_calls() {
        let mock = MockAtprotoClient::new();
        mock.get_record("app.bsky.feed.post", "abc123").unwrap();
        mock.assert_called("get_record", "app.bsky.feed.post/abc123", 1);
    }

    #[test]
    fn mock_returns_scripted_response() {
        let mock = MockAtprotoClient::new();
        mock.returns(
            "get_record",
            "app.bsky.feed.post/abc",
            serde_json::json!({"text": "hello"}),
        );
        let resp = mock.get_record("app.bsky.feed.post", "abc").unwrap();
        assert_eq!(resp["text"], "hello");
    }

    #[test]
    fn mock_put_record() {
        let mock = MockAtprotoClient::new();
        let record = serde_json::json!({"text": "new post"});
        mock.put_record("app.bsky.feed.post", &record).unwrap();
        mock.assert_called_any("put_record");
    }

    #[test]
    fn mock_delete_record() {
        let mock = MockAtprotoClient::new();
        mock.delete_record("app.bsky.feed.post", "abc").unwrap();
        mock.assert_called("delete_record", "app.bsky.feed.post/abc", 1);
    }

    #[test]
    fn mock_default_response_is_empty_object() {
        let mock = MockAtprotoClient::new();
        let resp = mock.get_record("unknown.collection", "key").unwrap();
        assert_eq!(resp, serde_json::json!({}));
    }

    #[test]
    fn mock_when_sets_null_initially() {
        let mock = MockAtprotoClient::new();
        mock.when("get_record", "app.bsky.feed.post/test");
        let resp = mock.get_record("app.bsky.feed.post", "test").unwrap();
        assert!(resp.is_null());
    }

    #[test]
    fn mock_list_records() {
        let mock = MockAtprotoClient::new();
        mock.returns(
            "list_records",
            "app.bsky.feed.post",
            serde_json::json!({"records": []}),
        );
        let resp = mock.list_records("app.bsky.feed.post").unwrap();
        assert!(resp["records"].is_array());
    }

    #[test]
    fn mock_calls_returns_all_calls() {
        let mock = MockAtprotoClient::new();
        mock.get_record("col1", "a").unwrap();
        mock.get_record("col2", "b").unwrap();
        mock.delete_record("col1", "a").unwrap();
        let calls = mock.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].method, "get_record");
        assert_eq!(calls[1].method, "get_record");
        assert_eq!(calls[2].method, "delete_record");
    }

    #[test]
    fn mock_put_record_captures_body() {
        let mock = MockAtprotoClient::new();
        let record = serde_json::json!({"text": "captured"});
        mock.put_record("app.bsky.feed.post", &record).unwrap();
        let calls = mock.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].body.as_ref().unwrap()["text"], "captured");
    }
}
