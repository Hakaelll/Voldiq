$ErrorActionPreference = 'Stop'
# Premise under review: moving a bin payload also retires its Shell metadata.
@(
    @{ actor = 'Direct physical transfer'; report = 'shell-direct-move-metadata-20261031-091922-642.json' },
    @{ actor = 'Observed physical move'; report = 'shell-observed-restore-metadata-20261031-092038-501.json' },
    @{ actor = 'Observed virtual-item move'; report = 'shell-virtual-restore-20261031-092136-678.json' },
    @{ actor = 'Windows undelete control'; report = 'shell-restore-control-comparison-20261031-093059-363.json'; branch = 'control' },
    @{ actor = 'Direct move with corrected criterion'; report = 'shell-restore-control-comparison-20261031-093059-363.json'; branch = 'move' }
) | ForEach-Object {
    $record = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot $_.report) | ConvertFrom-Json
    $observation = if ($_.ContainsKey('branch')) { $record.($_.branch).baseline } else { $record.observation.baseline }
    [pscustomobject]@{
        actor = $_.actor
        payloadRestored = $observation.restored
        metadataPresentBefore = $observation.binMetadataPresent
        metadataRemovedByMove = $observation.binMetadataRemoved
        ownOrphanCleanedByFixture = $observation.orphanMetadataCleaned
    }
} | ConvertTo-Json
