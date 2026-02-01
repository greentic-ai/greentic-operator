# PR: greentic-operator — Demo send builds `ChannelMessageEnvelope` with `to[]`; pass-through everywhere

## Summary
Update operator to:
- build `ChannelMessageEnvelope` with `to: [Destination{id, kind}]` for `demo send`
- stop any assumptions that destination is `user_id` or `channel`
- continue routing/passing envelopes without interpreting destination

## CLI contract
Add/standardize flags for demo send:

- `--to <id>` (repeatable or single; start with single)
- `--to-kind <kind>` (optional)

Examples:
- Webex room: `--to <roomId> --to-kind room`
- Teams channel: `--to <teamId>:<channelId> --to-kind channel`
- Email: `--to person@domain.com --to-kind email`
- Telegram: `--to -100123... --to-kind chat`

## Implementation steps

### 1) Envelope construction
Where demo send currently builds provider input JSON, replace with:

```rust
use greentic_types::{ChannelMessageEnvelope, Destination, Actor, TenantCtx};

let to = vec![Destination { id: to_id, kind: to_kind }];
let envelope = ChannelMessageEnvelope {
    id: /* stable id or uuid */,
    tenant: tenant_ctx,
    channel: provider_id_string, // keep meaning as provider/adapter channel id
    session_id: /* something deterministic for demo; often same as message id */,
    reply_scope: None,
    from: None, // demo send doesn't need sender
    correlation_id: None,
    text: Some(text),
    attachments: vec![],
    metadata: Default::default(),
    to,
};
let input_json = serde_json::to_vec(&envelope)?;
invoke_provider_op("send", input_json);
```

**Note:** `channel` should remain “adapter/provider channel id”, not destination.

### 2) Any routing store / reply logic
If you store sender or destination data in operator state:
- sender should be read from `envelope.from`
- destination should be read from `envelope.to`

But operator should *not* require either unless the provider op requires it.

### 3) Display / logging
Update any logging that prints `user_id` to print `from.id` where present.

## Tests
- Update any CLI integration tests that used old fields.
- Ensure `cargo test` passes.

Run:
```bash
cargo fmt
cargo test -p greentic-operator
```

## Acceptance criteria
- `demo send` supports `--to` and optional `--to-kind` and constructs envelope accordingly
- Operator compiles with updated greentic-types (no `user_id` references)
