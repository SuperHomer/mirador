# Mirador shell integration (regenerated on every launch — edits are lost).
# It does two things: reports the pane's cwd, and loads `mira`'s completions.
#
# PowerShell's `cd` moves its own location, not the *process* working
# directory, so Mirador cannot read a pane's cwd from the OS the way it does
# on macOS and Linux. Instead the prompt reports it with OSC 7, the same
# sequence VS Code, WezTerm and Ghostty use. That is what feeds the
# sidebar's cwd, git branch and PR status for this pane.
#
# The user's own prompt is preserved: this wraps it, and any failure inside
# the wrapper falls back to it silently.

if (-not $global:__MiradorPromptWrapped) {
    $global:__MiradorPromptWrapped = $true
    $global:__MiradorInnerPrompt = $function:prompt

    function global:prompt {
        try {
            $path = (Get-Location).ProviderPath
            if ($path) {
                # OSC 7: file://<host>/<percent-encoded path>, forward slashes.
                $encoded = [Uri]::EscapeDataString(($path -replace '\\', '/'))
                $encoded = $encoded -replace '%2F', '/' -replace '%3A', ':'
                $esc = [char]27
                [Console]::Write("$esc]7;file://$env:COMPUTERNAME/$encoded$esc\")
            }
        } catch {
            # Never let telemetry break someone's prompt.
        }
        & $global:__MiradorInnerPrompt
    }
}

# Tab completion for `mira`, from the `mira` on PATH so it matches the binary
# that will run. An old one without `completions` exits non-zero: then, or
# with no `mira` at all, nothing is loaded and nothing is shown.
if (-not $global:__MiradorCompletionsLoaded) {
    $global:__MiradorCompletionsLoaded = $true
    try {
        if (Get-Command mira -CommandType Application -ErrorAction SilentlyContinue) {
            # A child scope: this file runs at global scope, where a variable
            # of ours would be one of the person's.
            & {
                # The script is UTF-8 (its descriptions have em dashes);
                # PowerShell would read it in the console's OEM code page.
                $encoding = [Console]::OutputEncoding
                try {
                    [Console]::OutputEncoding = [Text.Encoding]::UTF8
                    $completions = mira completions powershell 2>$null | Out-String
                } finally {
                    [Console]::OutputEncoding = $encoding
                }
                if ($LASTEXITCODE -eq 0 -and $completions) {
                    Invoke-Expression $completions
                }
            }
            # An old mira's exit code would otherwise show in the first prompt.
            $global:LASTEXITCODE = 0
        }
    } catch {
        # A shell without completions is fine; a broken one is not.
    }
}
