import { describe, expect, it } from 'vitest';
import { existsSync, readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';

describe('Azure signing configuration', () => {
  it('validates settings without building or reading signing credentials', () => {
    const check = `
      $ErrorActionPreference = 'Stop'
      $tokens = $null; $parseErrors = $null
      $ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path (Get-Location) 'scripts/build-installer.ps1'), [ref]$tokens, [ref]$parseErrors)
      if ($parseErrors.Count) { throw 'Installer script has parse errors.' }
      $function = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'New-AzureSigningConfig' }, $true)
      if (-not $function) { throw 'Missing signing config validator.' }
      Invoke-Expression $function.Extent.Text
      $endpoint = 'https://example.codesigning.azure.net'
      $account = 'example-account'; $profile = 'example-profile'
      $config = New-AzureSigningConfig -Endpoint $endpoint -Account $account -Profile $profile
      $roundtrip = $config | ConvertTo-Json -Depth 8 | ConvertFrom-Json
      $command = $roundtrip.bundle.windows.signCommand
      if ($command.cmd -ne 'artifact-signing-cli' -or ($command.args -join '|') -ne "-e|$endpoint|-a|$account|-c|$profile|-d|PDF Workstation|%1") { throw 'Signing command changed.' }
      $cases = @(
        @('', $account, $profile), @($endpoint, '', $profile), @($endpoint, $account, ''),
        @('http://example.codesigning.azure.net', $account, $profile),
        @('https://example.codesigning.azure.net/path', $account, $profile),
        @('https://example.codesigning.azure.net?query=x', $account, $profile),
        @('https://example.codesigning.azure.net:443', $account, $profile),
        @('https://user@example.codesigning.azure.net', $account, $profile),
        @('https://example.invalid', $account, $profile),
        @($endpoint, '--argument', $profile), @($endpoint, $account, 'bad profile'),
        @($endpoint, $account, 'bad"profile'), @($endpoint, ('a' * 129), $profile)
      )
      foreach ($case in $cases) {
        $rejected = $false
        try { $null = New-AzureSigningConfig -Endpoint $case[0] -Account $case[1] -Profile $case[2] } catch {
          $rejected = $true
          if ($_.Exception.Message -like '*example*' -or $_.Exception.Message -like '*bad profile*') { throw 'Validation disclosed an input value.' }
        }
        if (-not $rejected) { throw 'Invalid signing input was accepted.' }
      }
      Write-Output 'Validated signing configuration and all negative controls.'
    `;
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(check, 'utf16le').toString('base64')], { encoding: 'utf8', timeout: 15_000 });
    expect(result.error).toBeUndefined();
    expect(result.status, result.stderr).toBe(0);
    expect(result.stdout).toContain('all negative controls');
  }, 20_000);

  it('uses automatically masked repository secrets and removes the generated override after the build', () => {
    const workflow = readFileSync('.github/workflows/release.yml', 'utf8');
    const installer = readFileSync('scripts/build-installer.ps1', 'utf8');
    for (const name of ['AZURE_SIGNING_ENDPOINT', 'AZURE_SIGNING_ACCOUNT', 'AZURE_SIGNING_PROFILE']) {
      expect(workflow.match(new RegExp(`secrets\\.${name}`, 'g'))).toHaveLength(2);
      expect(workflow).not.toContain(`vars.${name}`);
    }
    expect(workflow.indexOf('::add-mask::')).toBeLessThan(workflow.indexOf('npm ci'));
    expect(installer.indexOf('::add-mask::')).toBeLessThan(installer.indexOf('$azureConfig ='));
    expect(installer).toContain("'src-tauri/target/signing-config'");
    expect(installer).toContain('--config $configPath');
    expect(installer).toMatch(/finally\s*\{\s*if \(Test-Path -LiteralPath \$configPath -PathType Leaf\) \{ Remove-Item -LiteralPath \$configPath \}/);
    expect(installer).toContain('$buildExitCode = $LASTEXITCODE');
    expect(installer).toContain('verify-windows-signatures.ps1');
    expect(existsSync('src-tauri/tauri.azure.conf.json')).toBe(false);
  });
});
