# Bright shell integration for PowerShell.
#
# Bright starts PowerShell with -NoExit -Command, dot-sourcing this file after
# the user's profile has loaded. It adds the prompt marks: OSC 133 A (prompt
# start), B (prompt end), C (command start) and D with the exit status (command
# end), and OSC 7 with the working directory. No file of the user's is changed.

if ($global:__bright_marks) { return }
$global:__bright_marks = $true
$global:__bright_ran = $false
$global:__bright_prompt = $function:prompt
$esc = [char]27
$bel = [char]7

function global:prompt {
    $ok = $?
    $native = $global:LASTEXITCODE
    $e = [char]27
    $b = [char]7
    $out = ''
    if ($global:__bright_ran) {
        $code = if ($ok) { 0 } elseif ($native -is [int] -and $native -ne 0) { $native } else { 1 }
        $out += "$e]133;D;$code$b"
        $global:__bright_ran = $false
    }
    $cwd = (Get-Location).Path -replace '\\', '/'
    $encoded = [System.Uri]::EscapeUriString($cwd)
    if ($encoded -notmatch '^/') { $encoded = "/$encoded" }
    $hostName = [System.Net.Dns]::GetHostName()
    $out += "$e]7;file://$hostName$encoded$b$e]133;A$b"
    $user = if ($global:__bright_prompt) { & $global:__bright_prompt } else { "PS $((Get-Location).Path)> " }
    $global:LASTEXITCODE = $native
    return "$out$user$e]133;B$b"
}

# Command start: after a line is read and before it runs.
if (Get-Command Set-PSReadLineKeyHandler -ErrorAction SilentlyContinue) {
    Set-PSReadLineKeyHandler -Key Enter -ScriptBlock {
        [Microsoft.PowerShell.PSConsoleReadLine]::AcceptLine()
        $global:__bright_ran = $true
        [Console]::Out.Write("$([char]27)]133;C$([char]7)")
    }
}
