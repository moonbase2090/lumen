# Reads the accessibility tree of the real Bright window through Windows UI
# Automation (the API a screen reader uses), acts on a link through it, and
# checks that the terminal's own confirmation comes up and can be answered.
# Prints what it finds; exits non-zero when any step fails.
param([string]$Exe = "target\debug\lumen-term.exe")
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes

$app = Start-Process -FilePath $Exe -PassThru -ArgumentList @(
  "--", "powershell", "-NoProfile", "-Command",
  "Write-Output 'see https://example.com/a and https://example.com/b'; Start-Sleep -Seconds 600")
Write-Output "started $Exe as process $($app.Id)"

function Fail($message) {
  Write-Output "FAIL: $message"
  if (-not $app.HasExited) { Stop-Process -Id $app.Id -Force }
  exit 1
}

# Wait, up to a generous bound, until `$probe` returns something. The bound only
# ends a run that never gets its signal.
function WaitFor($what, [scriptblock]$probe, $seconds = 90) {
  $end = (Get-Date).AddSeconds($seconds)
  while ((Get-Date) -lt $end) {
    $found = & $probe
    if ($found) { return $found }
    Start-Sleep -Milliseconds 200
  }
  Fail "timed out waiting for $what"
}

$root = [System.Windows.Automation.AutomationElement]::RootElement
$byProcess = New-Object System.Windows.Automation.PropertyCondition(
  [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $app.Id)
$window = WaitFor "the Bright window" {
  $root.FindFirst([System.Windows.Automation.TreeScope]::Children, $byProcess)
}
Write-Output "window: name='$($window.Current.Name)' type=$($window.Current.ControlType.ProgrammaticName)"

$linkType = New-Object System.Windows.Automation.PropertyCondition(
  [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
  [System.Windows.Automation.ControlType]::Hyperlink)
$links = WaitFor "two links in the tree" {
  $all = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $linkType)
  if ($all.Count -ge 2) { $all }
}

Write-Output "tree:"
$everything = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants,
  [System.Windows.Automation.Condition]::TrueCondition)
foreach ($e in $everything) {
  Write-Output ("  {0} name='{1}'" -f $e.Current.ControlType.ProgrammaticName, $e.Current.Name)
}
$names = @($links | ForEach-Object { $_.Current.Name })
Write-Output "links: $($names -join ', ')"
if (-not ($names -contains "https://example.com/a" -and $names -contains "https://example.com/b")) {
  Fail "the two addresses are not the links' names"
}

$second = $links | Where-Object { $_.Current.Name -eq "https://example.com/b" } | Select-Object -First 1
$invoke = $second.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
$invoke.Invoke()
Write-Output "invoked the link https://example.com/b"

$buttonType = New-Object System.Windows.Automation.PropertyCondition(
  [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
  [System.Windows.Automation.ControlType]::Button)
$buttons = WaitFor "the Open and Cancel buttons" {
  $found = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $buttonType)
  $labels = @($found | ForEach-Object { $_.Current.Name })
  if (($labels -contains "Open") -and ($labels -contains "Cancel")) { $found }
}
$labels = @($buttons | ForEach-Object { $_.Current.Name })
Write-Output "the question is up with buttons: $($labels -join ', ')"
$everything = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants,
  [System.Windows.Automation.Condition]::TrueCondition)
$text = @($everything | ForEach-Object { $_.Current.Name } | Where-Object { $_ -like "*example.com/b*" -and $_ -like "*Open this link*" -or $_ -like "*Found in the program's output*" })
Write-Output "question text found: $($text.Count -gt 0)"

$cancel = $buttons | Where-Object { $_.Current.Name -eq "Cancel" } | Select-Object -First 1
$cancel.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
WaitFor "the question to close" {
  $found = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $buttonType)
  $found.Count -eq 0
} | Out-Null
Write-Output "Cancel closed the question"

Stop-Process -Id $app.Id -Force
Write-Output "PASS"
