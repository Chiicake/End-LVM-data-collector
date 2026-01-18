# End-LVM Data Collector

## CLI Dry-Run Pipeline
The `app` crate currently provides a dry-run CLI that writes a session using
pre-recorded input events and an optional raw frame.

### Run
```bash
cargo run -p app -- \
  --session-name 2026-01-18_10-30-00_run001 \
  --steps 30 \
  --dataset-root D:/dataset \
  --ffmpeg ffmpeg \
  --frame-raw path/to/frame.bgra \
  --events-jsonl path/to/events.jsonl \
  --thoughts-jsonl path/to/thoughts.jsonl
```

### Input Events JSONL
Each line is a single `InputEvent` JSON object.

Examples:
```json
{"qpc_ts":10,"type":"key_down","key":"W"}
{"qpc_ts":20,"type":"key_up","key":"W"}
{"qpc_ts":30,"type":"mouse_move","dx":12,"dy":-5}
{"qpc_ts":40,"type":"mouse_wheel","delta":120}
{"qpc_ts":50,"type":"mouse_button","button":"left","is_down":true}
```

Notes:
- `type` values: `key_down`, `key_up`, `mouse_move`, `mouse_wheel`, `mouse_button`.
- `button` values: `left`, `right`, `middle`, `x1`, `x2`.
- `qpc_ts` should be in the same units used by the pipeline; the CLI treats it
  as an opaque timestamp and slices windows using `step_index * 200ms`.
