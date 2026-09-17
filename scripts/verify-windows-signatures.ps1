param([Parameter(Mandatory = $true)][string[]]$Paths)
$ErrorActionPreference = 'Stop'
foreach ($path in $Paths) {
    $file = Get-Item -LiteralPath $path
    $signature = Get-AuthenticodeSignature -LiteralPath $file.FullName
    if ($signature.Status -ne 'Valid') { throw "$($file.Name): Authenticode signature is not valid ($($signature.Status))." }
    $publisher = $signature.SignerCertificate.GetNameInfo([Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false)
    if ($publisher -cne 'Joshua Beel') { throw "$($file.Name): unexpected certificate publisher." }
    if (-not $signature.TimeStamperCertificate) { throw "$($file.Name): signature has no timestamp." }
    Write-Output "$($file.Name): valid timestamped signature from Joshua Beel."
}
