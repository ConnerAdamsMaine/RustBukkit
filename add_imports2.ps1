$content = Get-Content 'crates/rustbukkit_bin/src/session.rs' -Raw

# 1. Add imports after world_persistence - try with CRLF
$content = $content -replace \"use crate::world_persistence::WorldStore;\r\n\", \"use crate::world_persistence::WorldStore;\r\nuse rustbukkit_chunks::{BlockKind, BlockPos, World};\r\nuse rustbukkit_protocol::encode_block_update;\r\n\"

[System.IO.File]::WriteAllText('crates/rustbukkit_bin/src/session.rs', $content)
Write-Host 'Imports added'
