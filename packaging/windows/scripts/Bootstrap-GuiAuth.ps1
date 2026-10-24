# Bootstrap Mini GUI admin credentials without putting the password on argv
# or under ProgramData. Expects a short-lived PassFile under Setup {tmp}.
#
# Cleanup is best-effort (overwrite + delete). It is NOT a forensic secure erase.
#
# Usage: Bootstrap-GuiAuth.ps1 -ExePath <exogpuagent.exe> -PassFile <temp file with one line>

param(
    [Parameter(Mandatory = $true)][string]$ExePath,
    [Parameter(Mandatory = $true)][string]$PassFile
)

$ErrorActionPreference = "Stop"
$exitCode = 1
try {
    if (-not (Test-Path -LiteralPath $ExePath)) {
        throw "exogpuagent.exe not found: $ExePath"
    }
    if (-not (Test-Path -LiteralPath $PassFile)) {
        throw "password file missing: $PassFile"
    }

    # Re-assert restrictive ACL (current user + SYSTEM) before reading.
    $user = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
    & icacls.exe $PassFile /inheritance:r /grant:r "${user}:F" /grant:r "*S-1-5-18:F" | Out-Null

    $pass = [System.IO.File]::ReadAllText($PassFile).TrimEnd("`r", "`n")
    if ($pass.Length -lt 12) {
        throw "password must be at least 12 characters"
    }

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $ExePath
    $psi.Arguments = "gui init --machine --password-stdin"
    $psi.UseShellExecute = $false
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.CreateNoWindow = $true

    $proc = [System.Diagnostics.Process]::Start($psi)
    $proc.StandardInput.WriteLine($pass)
    $proc.StandardInput.Close()
    $stdout = $proc.StandardOutput.ReadToEnd()
    $stderr = $proc.StandardError.ReadToEnd()
    $proc.WaitForExit()
    $exitCode = $proc.ExitCode
    if ($stdout) { Write-Output $stdout.TrimEnd() }
    if ($stderr) { [Console]::Error.WriteLine($stderr.TrimEnd()) }
    if ($exitCode -ne 0) {
        throw "gui init failed with exit code $exitCode"
    }
}
finally {
    $pass = $null
    if (Test-Path -LiteralPath $PassFile) {
        try {
            # Best-effort overwrite before delete (not forensic).
            $wipe = New-Object byte[] 256
            (New-Object Random).NextBytes($wipe)
            [System.IO.File]::WriteAllBytes($PassFile, $wipe)
        } catch { }
        Remove-Item -LiteralPath $PassFile -Force -ErrorAction SilentlyContinue
    }
}
exit $exitCode
