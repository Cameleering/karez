# Run cargo tests from Windows against the WSL test server without the WSL localhost relay.
#
# The relay (Windows 127.0.0.1 -> WSL) stalls when a test sends and receives several hundred KiB at
# once, so execute_many_does_not_deadlock_when_requests_and_results_are_both_large and
# a_failing_statement_in_a_large_batch_leaves_the_connection_usable hang for a minute and then fail
# with a connection reset. This script starts tests/docker/forward.py in WSL, points KAREZ_TEST_PG at
# the WSL VM's own address, runs cargo, and stops the forwarder.
#
#   docker compose -f tests/docker/compose.yaml up -d --wait     (in WSL, first)
#   powershell -File tests/docker/windows-test.ps1                            (all tests of the workspace)
#   powershell -File tests/docker/windows-test.ps1 test -p karez-pg --test pg_integration
#
# TLS tests verify the server name "localhost", so run those with the plain relay address, not this script.
[CmdletBinding(PositionalBinding = $false)]
param(
    [int]$TargetPort = 54316,
    [Parameter(ValueFromRemainingArguments = $true)] [string[]]$CargoArgs
)
$ErrorActionPreference = "Stop"
if (-not $CargoArgs -or $CargoArgs.Count -eq 0) { $CargoArgs = @("test", "--workspace") }
$listen = $TargetPort + 10
$ip = ((wsl -e hostname -I) -split "\s+")[0].Trim()
$script = (wsl -e wslpath -a (Join-Path $PSScriptRoot "forward.py")).Trim()
$fwd = Start-Process wsl -ArgumentList @("-e", "python3", $script, "$listen", "$TargetPort") -PassThru -WindowStyle Hidden
try {
    Start-Sleep -Seconds 2
    $env:KAREZ_TEST_PG = "${ip}:$listen"
    $env:KAREZ_REQUIRE_SERVER = "1"
    cargo @CargoArgs
    exit $LASTEXITCODE
}
finally {
    wsl -e pkill -f forward.py | Out-Null
    if (-not $fwd.HasExited) { Stop-Process -Id $fwd.Id -Force }
}
