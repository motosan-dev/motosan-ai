# Error Handling Policy

## Library Core

`motosan-ai` Rust SDK core keeps a strongly-typed public error surface via `MotosanError`.

- Public APIs return `Result<T, MotosanError>`
- Provider/status mapping stays explicit and testable
- Consumers can pattern-match by error kind (`Auth`, `RateLimit`, etc.)

## `anyhow` Evaluation

### Decision

Do **not** use `anyhow` in the library core.

### Rationale

- `anyhow::Error` is ergonomic for applications, but loses structured API-level error semantics.
- SDK users benefit from stable, typed error variants.
- Typed errors improve compatibility guarantees for downstream integrations.

### Allowed Usage

- `anyhow` may be used in `examples/` or CLI/application entry points only.
- If examples are added, they should convert from `MotosanError` at the boundary.

