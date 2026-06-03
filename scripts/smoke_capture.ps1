[CmdletBinding()]
param(
    [string]$ExePath = "",
    [string]$OutputPath = "",
    [int]$X = 80,
    [int]$Y = 80,
    [int]$Width = 1350,
    [int]$Height = 900,
    [int]$DelaySeconds = 5
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ExePath)) {
    $ExePath = Join-Path $PSScriptRoot "..\target\release\co2-reduction-finder.exe"
}
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $OutputPath = Join-Path $PSScriptRoot "..\target\smoke\co2-app-dwm-smoke.png"
}

$exe = Resolve-Path $ExePath
$output = [System.IO.Path]::GetFullPath($OutputPath)
$outputDir = Split-Path -Parent $output
New-Item -ItemType Directory -Force -Path $outputDir | Out-Null

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;

public class Co2SmokeCaptureNative {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);

    [DllImport("user32.dll")]
    public static extern bool MoveWindow(IntPtr hWnd, int x, int y, int width, int height, bool repaint);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("dwmapi.dll")]
    public static extern int DwmGetWindowAttribute(IntPtr hWnd, int attribute, out RECT rect, int attributeSize);
}
'@

$process = Start-Process -FilePath $exe -PassThru
try {
    $handle = [IntPtr]::Zero
    for ($i = 0; $i -lt 80; $i++) {
        Start-Sleep -Milliseconds 250
        $process.Refresh()
        if ($process.MainWindowHandle -ne 0) {
            $handle = $process.MainWindowHandle
            break
        }
    }

    if ($handle -eq [IntPtr]::Zero) {
        throw "Main window handle not found."
    }

    [Co2SmokeCaptureNative]::MoveWindow($handle, $X, $Y, $Width, $Height, $true) | Out-Null
    [Co2SmokeCaptureNative]::SetForegroundWindow($handle) | Out-Null
    Start-Sleep -Seconds $DelaySeconds

    $rect = New-Object Co2SmokeCaptureNative+RECT
    $rectSize = [Runtime.InteropServices.Marshal]::SizeOf([type]"Co2SmokeCaptureNative+RECT")
    $dwmResult = [Co2SmokeCaptureNative]::DwmGetWindowAttribute($handle, 9, [ref]$rect, $rectSize)
    if ($dwmResult -ne 0) {
        [Co2SmokeCaptureNative]::GetWindowRect($handle, [ref]$rect) | Out-Null
    }

    $captureWidth = [Math]::Max(1, $rect.Right - $rect.Left)
    $captureHeight = [Math]::Max(1, $rect.Bottom - $rect.Top)
    $bitmap = New-Object System.Drawing.Bitmap $captureWidth, $captureHeight
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
        $bitmap.Save($output, [System.Drawing.Imaging.ImageFormat]::Png)
    }
    finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }

    Write-Output "Captured $captureWidth x $captureHeight to $output"
}
finally {
    if ($process -and -not $process.HasExited) {
        $process.CloseMainWindow() | Out-Null
        Start-Sleep -Milliseconds 500
        if (-not $process.HasExited) {
            $process.Kill()
        }
    }
}
