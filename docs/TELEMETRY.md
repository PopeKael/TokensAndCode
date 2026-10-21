# Codex telemetry notes

Tokens and Code reads Codex rollout JSONL records without changing them. It supports plain `.jsonl` and Zstandard-compressed `.jsonl.zst` files named `rollout-*`.

## Thread and turn relationships

- `session_meta.payload.id` identifies the thread represented by a rollout file. `payload.session_id` groups threads into a Codex session when present; otherwise the thread id is used.
- `payload.parent_thread_id` links a child thread to its parent. The scanner follows this relationship to group Guardian and other subagent work under the primary thread. A thread marked `thread_source: "user"` is primary. Guardian/review source labels and other non-user sources are shown separately as child work.
- `turn_id` identifies the turn in a thread. `root_turn_id` is a cross-thread turn association when present; it does not replace the parent-thread relationship.
- `response_id` identifies an atomic `token_usage_record` response. The scanner skips a duplicate non-empty response id if it appears more than once in the scanned tree.
- A `turn_context` record supplies model, reasoning effort, and cwd for its `turn_id`. The nearest context at or before the usage record is selected. If no usable context exists, the app shows an unknown value and falls back to the thread cwd where possible.

## Token accounting

`token_usage_record.payload.usage` is treated as the atomic response usage. The app does not also add the record's accompanying `thread_token_usage` or later cumulative snapshots.

For a legacy thread without atomic usage, `event_msg` records with `payload.type: "token_count"` provide cumulative `info.total_token_usage`. The scanner differences successive cumulative values to avoid adding the same earlier tokens again. These inferred deltas are useful for daily charts, but they are not true response records and may be less precise than atomic usage. Missing or decreasing counters produce a zero delta for that field.

Fresh input is `max(input_tokens - cached_input_tokens, 0)`. Cache-write input is a subset of fresh input and is priced separately. Reasoning output is already included in output and is not added again. If `total_tokens` is missing or zero, input plus output is used as a fallback.

## Time, model, and path

Event envelope timestamps are the primary usage timestamps. The scanner converts them to Asia/Bangkok for period totals and history. A missing usage timestamp falls back to the thread metadata time, then scan time. Model, effort, and cwd are read from turn context; the thread metadata cwd is a fallback. The project label is the last path component of cwd. Local Git state is not required and is not used to rewrite historical records.

## Plus allowance snapshots

`token_count` rate-limit payloads can include `plan_type`, `primary` and/or `secondary` windows, `used_percent`, `window_minutes`, and Unix-epoch `resets_at`. The app keeps the latest Plus record it sees for 300-minute and 10,080-minute windows. This is a snapshot recorded in local logs, not a live account query.

## Format differences

Rollouts vary by age and source. Some have `thread_source`; others identify the source as a string or a nested `source.subagent` object. Some legacy threads have only cumulative `token_count` records, while newer threads can have atomic usage records. Context fields and timestamps can be absent. The scanner tolerates missing fields and labels unavailable context instead of inferring a model or effort.
