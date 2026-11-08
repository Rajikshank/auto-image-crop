$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$destination = Join-Path $root 'models'
New-Item -ItemType Directory -Force -Path $destination | Out-Null
function Get-VerifiedFile([string]$Url, [string]$File, [string]$Hash) {
    $path = Join-Path $destination $File
    if (!(Test-Path -LiteralPath $path) -or (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $Hash) {
        $partial = "$path.partial"
        Invoke-WebRequest -Uri $Url -OutFile $partial
        if ((Get-FileHash -LiteralPath $partial -Algorithm SHA256).Hash -ne $Hash) { throw "Checksum mismatch: $File" }
        Move-Item -LiteralPath $partial -Destination $path -Force
    }
    Write-Host "Verified $File"
}
$revision = '0d3b403339b4674a82493d5e97964dd78089ddc8'
Get-VerifiedFile "https://huggingface.co/Acly/MobileSAM/resolve/$revision/mobile_sam_image_encoder.onnx" 'mobile_sam_image_encoder.onnx' '580F5FB648EA1062C0AABC26217AED56921985F03F0CBBD852BBA81D760CC749'
Get-VerifiedFile "https://huggingface.co/Acly/MobileSAM/resolve/$revision/sam_mask_decoder_multi.onnx" 'sam_mask_decoder_multi.onnx' '8976B90A87BA50A6A72217A5FF994F7D25CE16F2229FCC1ED259E1294C622FFE'
Get-VerifiedFile 'https://www.nuget.org/api/v2/package/Microsoft.ML.OnnxRuntime.DirectML/1.24.2' 'runtime.zip' 'C9B8ADB96DFB5578097BEA42A7D9B7FF8F300FB3C3A6F3052FE5B702628AB681'
Get-VerifiedFile 'https://www.nuget.org/api/v2/package/Microsoft.AI.DirectML/1.15.4' 'directml.zip' '4E7CB7DDCE8CF837A7A75DC029209B520CA0101470FCDF275C1F49736A3615B9'
Expand-Archive -LiteralPath (Join-Path $destination 'runtime.zip') -DestinationPath (Join-Path $destination 'runtime') -Force
Expand-Archive -LiteralPath (Join-Path $destination 'directml.zip') -DestinationPath (Join-Path $destination 'directml') -Force
Copy-Item -LiteralPath (Join-Path $destination 'directml/bin/x64-win/DirectML.dll') -Destination (Join-Path $destination 'runtime/runtimes/win-x64/native/DirectML.dll') -Force
Write-Host 'Local models and runtime are ready. No screenshot is uploaded by ObjectLift.'
