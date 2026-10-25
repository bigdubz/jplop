# Fetch all required assets for jplop
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = $PSScriptRoot
Set-Location $Root

$ModelDir = "models/lfm"
$ModelFile = "$ModelDir/lfmf16.gguf"
$ModelUrl = "https://huggingface.co/LiquidAI/LFM2-350M-ENJP-MT-GGUF/resolve/main/LFM2-350M-ENJP-MT-F16.gguf?download=true"
$ModelSha256 = "0ed20b653006d4e97dd2b4a22872971e45fa71ad6ccb3cd21705747ee08d3b30"

$JmGz = "JMdict_e_examp.gz"
$JmUrl = "http://ftp.edrdg.org/pub/Nihongo/JMdict_e_examp.gz"
$JmFile = "jm.xml"

$LlamaBuild = "b11856"
$LlamaDir = "tools/llama.cpp"
$LlamaReleaseUrl = "https://github.com/ggml-org/llama.cpp/releases/download/$LlamaBuild"

function Download-File {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Url,

        [Parameter(Mandatory = $true)]
        [string]$Output
    )

    $OutputPath = Join-Path $Root $Output

    if (Test-Path -LiteralPath $OutputPath -PathType Leaf) {
        Write-Host "Already exists: $Output"
        return
    }

    $TempPath = "$OutputPath.part"

    if (Test-Path -LiteralPath $TempPath) {
        Remove-Item -LiteralPath $TempPath -Force
    }

    Write-Host "Downloading $Output ..."

    & curl.exe `
        --fail `
        --location `
        --retry 3 `
        --retry-delay 2 `
        --output $TempPath `
        --progress-bar `
        $Url

    if ($LASTEXITCODE -ne 0) {
        Remove-Item -LiteralPath $TempPath -Force -ErrorAction SilentlyContinue
        throw "Download failed: $Output (curl exit code $LASTEXITCODE)"
    }

    Move-Item -LiteralPath $TempPath -Destination $OutputPath -Force
}

function Test-Sha256 {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$Expected
    )

    Write-Host "Verifying SHA-256: $Path"
    $Actual = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()

    if ($Actual -ne $Expected.ToLowerInvariant()) {
        throw "SHA-256 mismatch for $Path. Expected $Expected, got $Actual. Delete the file and rerun fetch.ps1."
    }

    Write-Host "SHA-256 OK: $Path"
}


function Expand-Zip {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ArchivePath,

        [Parameter(Mandatory = $true)]
        [string]$Destination,

        [switch]$Flatten
    )

    $ArchiveFullPath = Join-Path $Root $ArchivePath
    $DestinationFullPath = Join-Path $Root $Destination

    if (-not (Test-Path -LiteralPath $ArchiveFullPath -PathType Leaf)) {
        throw "ZIP archive not found: $ArchiveFullPath"
    }

    if ($Flatten) {
        $ExtractDir = Join-Path $Root "$Destination-extracted"

        if (Test-Path -LiteralPath $ExtractDir) {
            Remove-Item -LiteralPath $ExtractDir -Recurse -Force
        }

        New-Item -ItemType Directory -Path $ExtractDir -Force | Out-Null
        Expand-Archive -LiteralPath $ArchiveFullPath -DestinationPath $ExtractDir -Force

        $DictionaryFile = Get-ChildItem -LiteralPath $ExtractDir -Filter "system_core.dic" -File -Recurse |
            Select-Object -First 1

        if ($null -eq $DictionaryFile) {
            throw "system_core.dic was not found in $ArchivePath"
        }

        Copy-Item -LiteralPath $DictionaryFile.FullName -Destination (Join-Path $Root "system.dic") -Force
        Remove-Item -LiteralPath $ExtractDir -Recurse -Force
    }
    else {
        New-Item -ItemType Directory -Path $DestinationFullPath -Force | Out-Null
        Expand-Archive -LiteralPath $ArchiveFullPath -DestinationPath $DestinationFullPath -Force
    }
}


# Ensure .NET ZIP support is loaded.
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem


#1. Sudachi dictionary
$SystemDic = Join-Path $Root "system.dic"

if (-not (Test-Path -LiteralPath $SystemDic -PathType Leaf)) {
    Write-Host "Fetching Sudachi system dictionary..."

    $DictVersion = "latest"
    $DictType = "core"
    $DictFormat = "v1"
    $DictName = "sudachi-dictionary-$DictVersion-$DictType"
    $DictZip = "$DictName.zip"
    $DictUrl = "https://d2ej7fkh96fzlu.cloudfront.net/sudachidict/$DictFormat/$DictZip"

    try {
        Download-File -Url $DictUrl -Output $DictZip
        Expand-Zip -ArchivePath $DictZip -Destination $DictName -Flatten
    }
    finally {
        Remove-Item -LiteralPath (Join-Path $Root $DictZip) -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath (Join-Path $Root $DictName) -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath (Join-Path $Root "$DictName-extracted") -Recurse -Force -ErrorAction SilentlyContinue
    }
}
else {
    Write-Host "Already exists: system.dic"
}

# 2. Translation model
New-Item -ItemType Directory -Force -Path $ModelDir | Out-Null

if (-not (Test-Path -LiteralPath $ModelFile -PathType Leaf)) {
    Download-File -Url $ModelUrl -Output $ModelFile
}
else {
    Write-Host "Already exists: $ModelFile"
}

Test-Sha256 -Path $ModelFile -Expected $ModelSha256

# 3. JMdict English dictionary with example sentences
if (-not (Test-Path -LiteralPath $JmFile -PathType Leaf)) {
    Download-File -Url $JmUrl -Output $JmGz

    $JmGzPath = Join-Path $Root $JmGz
    $JmTempPath = Join-Path $Root "$JmFile.part"
    $JmOutputPath = Join-Path $Root $JmFile

    Remove-Item -LiteralPath $JmTempPath -Force -ErrorAction SilentlyContinue

    Write-Host "Extracting JMdict..."

    try {
        $InputStream = [System.IO.File]::OpenRead($JmGzPath)
        try {
            $GzipStream = [System.IO.Compression.GzipStream]::new(
                $InputStream,
                [System.IO.Compression.CompressionMode]::Decompress
            )
            try {
                $OutputStream = [System.IO.File]::Create($JmTempPath)
                try {
                    $GzipStream.CopyTo($OutputStream)
                }
                finally {
                    $OutputStream.Dispose()
                }
            }
            finally {
                $GzipStream.Dispose()
            }
        }
        finally {
            $InputStream.Dispose()
        }

        Move-Item -LiteralPath $JmTempPath -Destination $JmOutputPath -Force
        Remove-Item -LiteralPath $JmGzPath -Force
    }
    catch {
        Remove-Item -LiteralPath $JmTempPath -Force -ErrorAction SilentlyContinue
        throw "Could not decompress JMdict: $($_.Exception.Message)"
    }
}
else {
    Write-Host "Already exists: $JmFile"
}

# 4. llama.cpp CPU build
$CpuServer = Join-Path $Root "$LlamaDir/cpu/llama-server.exe"
$CpuZip = "llama-$LlamaBuild-bin-win-cpu-x64.zip"

if (-not (Test-Path -LiteralPath $CpuServer -PathType Leaf)) {
    try {
        Download-File -Url "$LlamaReleaseUrl/$CpuZip" -Output $CpuZip
        Expand-Zip -ArchivePath $CpuZip -Destination "$LlamaDir/cpu"
    }
    finally {
        Remove-Item -LiteralPath (Join-Path $Root $CpuZip) -Force -ErrorAction SilentlyContinue
    }
}
else {
    Write-Host "Already exists: $CpuServer"
}

# 5. llama.cpp CUDA build
$CudaServer = Join-Path $Root "$LlamaDir/cuda/llama-server.exe"
$CudaZip = "llama-$LlamaBuild-bin-win-cuda-12.4-x64.zip"

if (-not (Test-Path -LiteralPath $CudaServer -PathType Leaf)) {
    try {
        Download-File -Url "$LlamaReleaseUrl/$CudaZip" -Output $CudaZip
        Expand-Zip -ArchivePath $CudaZip -Destination "$LlamaDir/cuda"
    }
    finally {
        Remove-Item -LiteralPath (Join-Path $Root $CudaZip) -Force -ErrorAction SilentlyContinue
    }
}
else {
    Write-Host "Already exists: $CudaServer"
}

# 6. CUDA runtime DLLs (distributed separately)
$CudaRuntimeZip = "cudart-llama-bin-win-cuda-12.4-x64.zip"

try {
    # Always download and extract this archive to restore any missing runtime DLLs.
    Download-File -Url "$LlamaReleaseUrl/$CudaRuntimeZip" -Output $CudaRuntimeZip
    Expand-Zip -ArchivePath $CudaRuntimeZip -Destination "$LlamaDir/cuda"
}
finally {
    Remove-Item -LiteralPath (Join-Path $Root $CudaRuntimeZip) -Force -ErrorAction SilentlyContinue
}

# 7. Verify required llama.cpp files
$RequiredFiles = @(
    "$LlamaDir/cpu/llama-server.exe",
    "$LlamaDir/cuda/llama-server.exe",
    "$LlamaDir/cuda/ggml-cuda.dll",
    "$LlamaDir/cuda/cublas64_12.dll",
    "$LlamaDir/cuda/cublasLt64_12.dll",
    "$LlamaDir/cuda/cudart64_12.dll"
)

foreach ($File in $RequiredFiles) {
    $FullPath = Join-Path $Root $File

    if (-not (Test-Path -LiteralPath $FullPath -PathType Leaf)) {
        throw "Required llama.cpp file is missing: $FullPath"
    }
}

Write-Host ""
Write-Host "llama.cpp CPU and CUDA builds are ready."
Write-Host ""
Write-Host "All assets are ready."
