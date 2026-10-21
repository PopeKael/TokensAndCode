# Tokens and Code — Windows x64 portable release

## Download and run

Download the versioned `TokensAndCode-*-windows-x64.zip` asset, extract the whole archive, then open `TokensAndCode.exe` from the extracted folder. The archive contains the executable, README and MIT licence. A separate `.sha256` file is provided to verify the ZIP.

On first launch, choose or confirm the Codex sessions folder, approve read-only access, and select **Save setup and refresh**. The default folder is `%USERPROFILE%\.codex\sessions`.

This is an unsigned portable application. It does not install itself or create a Start Menu shortcut, and it requires no administrator privileges. Source code and build instructions are in this repository.

## What it does

Tokens and Code scans Codex rollout logs locally and displays token usage, daily trends, Plus allowance snapshots recorded in the logs, and hypothetical API-equivalent comparisons. It does not modify the selected source logs or upload their contents.

See the [README](https://github.com/PopeKael/TokensAndCode/blob/main/README.md) for details and [telemetry notes](https://github.com/PopeKael/TokensAndCode/blob/main/docs/TELEMETRY.md) for the fields it reads. Suggestions and bug reports are welcome in [GitHub Issues](https://github.com/PopeKael/TokensAndCode/issues).
