# Turns a Windows High Contrast theme on and off under a running Bright window and
# reads the palette the window draws each time, from a screenshot of it: the
# divider of the trust margin is one exact colour in the light palette and
# another in the high-contrast one. Also checks that the window keeps the focus
# while it reads the setting (the read must not open a console, which would take
# the focus), and runs the contrast test against the system while the theme is on.
# Prints what it finds; exits non-zero when any step fails.
param([string]$Exe = "target\debug\lumen-term.exe")
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing, System.Windows.Forms
Add-Type -ReferencedAssemblies System.Drawing @"
using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
public static class Probe {
  [StructLayout(LayoutKind.Sequential)]
  public struct HIGHCONTRAST { public uint cbSize; public uint dwFlags; public IntPtr lpszDefaultScheme; }
  [DllImport("user32.dll", SetLastError = true, EntryPoint = "SystemParametersInfoW")]
  static extern bool Spi(uint action, uint size, ref HIGHCONTRAST value, uint flags);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
  public static bool SetHighContrast(bool on) {
    HIGHCONTRAST h = new HIGHCONTRAST();
    h.cbSize = (uint)Marshal.SizeOf(typeof(HIGHCONTRAST));
    h.dwFlags = on ? 1u : 0u;
    h.lpszDefaultScheme = Marshal.StringToHGlobalUni("High Contrast Black");
    // SPI_SETHIGHCONTRAST, with SPIF_UPDATEINIFILE | SPIF_SENDCHANGE.
    return Spi(0x43, h.cbSize, ref h, 3);
  }
  public static uint ForegroundPid() { uint pid; GetWindowThreadProcessId(GetForegroundWindow(), out pid); return pid; }
  // How many pixels of the capture are exactly this colour.
  public static int Count(Bitmap bmp, byte r, byte g, byte b) {
    Rectangle rect = new Rectangle(0, 0, bmp.Width, bmp.Height);
    BitmapData data = bmp.LockBits(rect, ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
    byte[] bytes = new byte[data.Stride * data.Height];
    Marshal.Copy(data.Scan0, bytes, 0, bytes.Length);
    bmp.UnlockBits(data);
    int n = 0;
    for (int y = 0; y < data.Height; y++)
      for (int x = 0; x < data.Width; x++) {
        int i = y * data.Stride + x * 4;
        if (bytes[i] == b && bytes[i + 1] == g && bytes[i + 2] == r) n++;
      }
    return n;
  }
}
"@

$app = Start-Process -FilePath $Exe -PassThru -ArgumentList @(
  "--", "powershell", "-NoProfile", "-Command",
  "Write-Output 'Contrast check'; Start-Sleep -Seconds 900")
$other = $null
Write-Output "started $Exe as process $($app.Id)"

function Finish($code) {
  [Probe]::SetHighContrast($false) | Out-Null
  if (-not $app.HasExited) { Stop-Process -Id $app.Id -Force }
  if ($other -and -not $other.HasExited) { Stop-Process -Id $other.Id -Force }
  exit $code
}
function Fail($message) { Write-Output "FAIL: $message"; Finish 1 }

# Wait until `$probe` returns something; the bound only ends a run that never gets its signal.
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

# What palette the window draws now, from a capture of its own rectangle.
function Palette() {
  $r = $window.Current.BoundingRectangle
  $bmp = New-Object System.Drawing.Bitmap ([int]$r.Width), ([int]$r.Height)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen([int]$r.X, [int]$r.Y, 0, 0, $bmp.Size)
  $g.Dispose()
  $light = [Probe]::Count($bmp, 222, 225, 231)
  $hc = [Probe]::Count($bmp, 140, 140, 140)
  $bmp.Dispose()
  # A few stray pixels can match by chance (window frame, glyph edges), so the
  # palette is the one whose divider colour is at least ten times as common.
  if ($light -gt 0 -and $light -ge (10 * $hc)) { return "light" }
  if ($hc -gt 0 -and $hc -ge (10 * $light)) { return "high-contrast" }
  return "unclear (light=$light, high-contrast=$hc)"
}
function WaitForPalette($want) {
  $end = (Get-Date).AddSeconds(60)
  $got = ""
  while ((Get-Date) -lt $end) {
    $got = Palette
    if ($got -eq $want) { return $got }
    Start-Sleep -Milliseconds 300
  }
  Fail "the window draws '$got', not '$want'"
}

# The window reads the setting when it gains focus: take the focus away, give it back.
function RefocusAndCheck($label) {
  $script:other = Start-Process notepad.exe -PassThru
  WaitFor "notepad to have the focus" { [Probe]::ForegroundPid() -eq $script:other.Id } | Out-Null
  (New-Object -ComObject WScript.Shell).AppActivate($app.Id) | Out-Null
  WaitFor "the Bright window to have the focus" { [Probe]::ForegroundPid() -eq $app.Id } | Out-Null
  # Read the setting a few more times over a few seconds: a console opened by the
  # read would take the focus, and this would see the other process in front.
  for ($i = 0; $i -lt 10; $i++) {
    Start-Sleep -Milliseconds 300
    if ([Probe]::ForegroundPid() -ne $app.Id) { Fail "${label}: the window lost the focus" }
  }
  Write-Output "${label}: the window has the focus and keeps it"
  Stop-Process -Id $script:other.Id -Force
  $script:other = $null
}

(New-Object -ComObject WScript.Shell).AppActivate($app.Id) | Out-Null
WaitFor "the window to have the focus" { [Probe]::ForegroundPid() -eq $app.Id } | Out-Null
Write-Output "start (theme off): $(WaitForPalette 'light')"

if (-not [Probe]::SetHighContrast($true)) { Fail "could not turn High Contrast on" }
Write-Output "High Contrast turned on"
RefocusAndCheck "on"
Write-Output "after refocus: $(WaitForPalette 'high-contrast')"

Write-Output "contrast test with the theme on:"
cargo test --locked -p lumen-term --test contrast_windows -- --nocapture
if ($LASTEXITCODE -ne 0) { Fail "the contrast test failed with the theme on" }

if (-not [Probe]::SetHighContrast($false)) { Fail "could not turn High Contrast off" }
Write-Output "High Contrast turned off"
RefocusAndCheck "off"
Write-Output "after refocus: $(WaitForPalette 'light')"

Write-Output "PASS"
Finish 0
