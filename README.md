# Tokens and Code

Tokens and Code is a small Windows tray utility for exploring Codex token telemetry stored on your own PC. It scans Codex rollout logs locally, keeps an aggregate recent copy for quick opening, and lets you refresh that copy when you choose.

## What it does

- Reads `.jsonl` and `.jsonl.zst` rollout files recursively from a folder you select.
- Uses atomic `token_usage_record` entries when present. It uses cumulative `token_count` records only for older threads without atomic usage, calculating deltas so cumulative snapshots are not added repeatedly.
- Associates child threads with their primary session and labels Guardian and other subagent work separately.
- Shows current session, today, this week, this month, custom date ranges, and all-time totals in Asia/Bangkok time.
- Breaks out input, cached input, fresh input, output, and reasoning output. Reasoning output is part of output and is not charged twice.
- Shows a daily token chart, Plus allowance snapshots recorded in the logs, and project/session/model breakdowns.
- Provides hypothetical Standard API-equivalent token costs for GPT-6 Astra, GPT-6 Sol (currently GPT-6.1 Sol in the API), and GPT-6 Luna.
- Shows a separate Artificial Analysis Intelligence Index comparison. Its cost-per-task metric is benchmark context; it does not alter the token-based estimate.

The app does not edit the selected rollout logs and does not upload them. The selected folder is read only. Your source path, current snapshot, and settings stay in your Windows local application data folder. The recent snapshot contains aggregate token records and local project/session identifiers, so keep it private like your Codex logs.

For the fields and relationships extracted from Codex rollouts, see [the telemetry notes](docs/TELEMETRY.md).

## Recent copy and refresh

After you approve the selected folder in Setup, the first scan runs in the background. Tokens and Code saves a local recent snapshot when the scan finishes. Later launches show that snapshot immediately. Use **Refresh** in the window or **Refresh logs** in the tray menu to rescan. After 24 hours, the app marks the displayed copy stale and offers **Refresh now**. Full scans of large or older log trees can take time; progress is shown in the app.

Closing the window hides it in the system tray. Use the tray menu's **Exit** command to quit. Start the executable again from Windows to bring the tray utility back; automatic Windows startup is not currently configured.

## Install and run

Download `TokensAndCode.exe` from a GitHub Release and run it. On first launch:

1. Confirm or choose the Codex sessions folder.
2. Approve read-only access to that local folder.
3. Select **Save setup and refresh**.

By default the folder is `%USERPROFILE%\.codex\sessions`. No administrator access is required.

## Build from source

Requirements: Windows 10 or newer and the stable Rust toolchain.

```powershell
cargo build --release --locked
```

The portable executable is `target\release\TokensAndCode.exe`. The app uses native Rust/egui UI, an embedded Windows application icon, and a system tray icon; no browser runtime, Python installation, database server, or external web assets are required.

The portable ZIP does not install the app or create a Start Menu entry. The executable's embedded icon is ready for a Windows shortcut once a permanent run location is chosen; the install and shortcut flow remains a separate packaging step.

## Where local data is stored

Settings and the recent snapshot are stored under the Windows Local AppData directory provided by the OS for Tokens and Code. Use **Setup** to change the source folder. Removing the app does not automatically remove this local data; delete the Tokens and Code application data folder manually if you want to clear it.

## Cost and benchmark notes

The API-equivalent estimates use published Standard token prices and the log's input, cached input, cache-write input, and output counts. A response with more than 272,000 input tokens uses published long-context rates for the full response. Cache writes use the 30-minute cache-write rate because the Codex log does not identify cache-write duration. Tool calls, service charges, taxes, and provider-specific adjustments are excluded. These are comparisons, not invoices or charges made by this app.

Prices checked 21 October 2026 against [OpenAI API pricing](https://developers.openai.com/api/docs/pricing) and [GPT-6 Astra model details](https://developers.openai.com/api/docs/models/gpt-6-astra). Artificial Analysis figures use the Max variants and Intelligence Index v4.3.2, checked 21 October 2026: GPT-6 Astra 53 / $3.26 per task, GPT-6.1 Sol 52 / $0.72 per task, and GPT-6 Luna 38 / $0.07 per task. These benchmark costs are independent of the selected token period. See [Artificial Analysis](https://artificialanalysis.ai/evaluations/artificial-analysis-intelligence-index) for methodology. Published prices and benchmark results can change; refresh these documented values before future releases.

## Privacy

Tokens and Code processes the selected files locally. It has no analytics, network scanner, or telemetry upload. The source files are opened read only. Hyperlinks in the benchmark panel open the referenced pages only when clicked.

## Licence

MIT. See [LICENSE](LICENSE).
