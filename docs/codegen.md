# Lexicon Code Generation

Generate Rust types and handler stubs from your AT Protocol lexicon JSON files.

## Quick Start

1. Create a `lexicons/` directory with your `.json` files
2. Run `atrg generate`
3. Add `mod generated;` to your code

## Example

Given `lexicons/com.example.ping.json`:

```json
{
  "lexicon": 1,
  "id": "com.example.ping",
  "defs": {
    "main": {
      "type": "query",
      "output": {
        "encoding": "application/json",
        "schema": {
          "type": "object",
          "required": ["pong"],
          "properties": {
            "pong": { "type": "boolean" }
          }
        }
      }
    }
  }
}
```

Running `atrg generate` produces `src/generated/types.rs` with a `ComExamplePingOutput` struct and `src/generated/routes.rs` with handler stubs.

## Options

```bash
atrg generate --input lexicons/ --output src/generated/
```
