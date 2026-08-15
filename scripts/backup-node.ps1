param(
    [Parameter(Mandatory = $true)]
    [string]$DataDir,
    [Parameter(Mandatory = $true)]
    [string]$Destination
)

$resolvedData = (Resolve-Path -LiteralPath $DataDir -ErrorAction Stop).Path
$parent = Split-Path -Parent $Destination
if ($parent) {
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
}
if (Test-Path -LiteralPath $Destination) {
    throw "Backup destination already exists: $Destination"
}

Compress-Archive -LiteralPath $resolvedData -DestinationPath $Destination -CompressionLevel Optimal
Write-Output "Created node backup: $Destination"
