# Captures every Buginotch page in one look (solid, dark glass or light glass) over one backdrop.
# Called by scripts/ui-check.sh. Needs a Windows desktop; it moves the mouse and takes screenshots.
param([string]$Exe, [string]$Out, [string]$Look, [string]$Backdrop, [int]$Pages = 12)
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type -Name W -Namespace N -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y); [DllImport("user32.dll")] public static extern void mouse_event(int f,int x,int y,int d,int e);'
function Click($x,$y){[N.W]::SetCursorPos($x,$y);Start-Sleep -Milliseconds 120;[N.W]::mouse_event(2,0,0,0,0);Start-Sleep -Milliseconds 60;[N.W]::mouse_event(4,0,0,0,0)}
$sw = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds.Width
$cx = [int]($sw / 2)
New-Item -ItemType Directory -Force -Path $Out | Out-Null

# The backdrop: a flat grey (for the automatic checks) or coloured stripes (for the eye).
$bgScript = @"
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
`$f=New-Object System.Windows.Forms.Form
`$f.FormBorderStyle='None';`$f.StartPosition='Manual';`$f.Location='$($cx-460),0';`$f.Size='920,330';`$f.ShowInTaskbar=`$false
`$mode='$Backdrop'
`$f.Add_Paint({param(`$s,`$e)
 if(`$mode -eq 'flat'){`$e.Graphics.Clear([System.Drawing.ColorTranslator]::FromHtml('#9aa0aa'))}
 else{`$cols=@('#ff3b30','#ffffff','#1f6bff','#ffd60a','#30d158','#000000')
  for(`$i=0;`$i -lt 19;`$i++){`$b=New-Object System.Drawing.SolidBrush ([System.Drawing.ColorTranslator]::FromHtml(`$cols[`$i%6]));`$e.Graphics.FillRectangle(`$b,`$i*50,0,50,330)}}})
`$t=New-Object System.Windows.Forms.Timer;`$t.Interval=90000;`$t.Add_Tick({`$f.Close()});`$t.Start()
[System.Windows.Forms.Application]::Run(`$f)
"@
$bgFile = Join-Path $env:TEMP 'buginotch-uicheck-bg.ps1'
Set-Content -Path $bgFile -Value $bgScript
$bg = Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',$bgFile -PassThru
Start-Sleep 3

$env:ISLE_FAKE_UPDATE = 'available'
$env:ISLE_PAGE_ORDER = 'music,timer,todo,clipboard,shelf,water,widgets,notifications,digioh,usage,update'
Remove-Item Env:ISLE_GLASS -ErrorAction SilentlyContinue
if ($Look -eq 'dark') { $env:ISLE_GLASS = '1' }
if ($Look -eq 'light') { $env:ISLE_GLASS = 'light' }
$app = Start-Process $Exe -PassThru
Start-Sleep 4
[N.W]::SetCursorPos($cx, 4); Start-Sleep -Milliseconds 1800
for ($i = 0; $i -lt $Pages; $i++) {
  $bmp = New-Object System.Drawing.Bitmap 800,270
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($cx - 400, 0, 0, 0, $bmp.Size)
  $bmp.Save((Join-Path $Out ("{0}_{1}_{2:00}.png" -f $Look, $Backdrop, $i)))
  $g.Dispose(); $bmp.Dispose()
  Click ($cx + 248) 92; [N.W]::SetCursorPos($cx, 60); Start-Sleep -Milliseconds 900
}
[N.W]::SetCursorPos($cx, 500)
Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
Stop-Process -Id $bg.Id -Force -ErrorAction SilentlyContinue
