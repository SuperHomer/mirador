# Mirador shell integration (regenerated on every launch — edits are lost).
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
