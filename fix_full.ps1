$content = Get-Content 'crates/rustbukkit_bin/src/session.rs' -Raw

# Fix the imports properly - put them after world_persistence::WorldStore and before BlockEvent
$content = $content -replace "use crate::world_persistence::WorldStore;`n", "use crate::world_persistence::WorldStore;`nuse rustbukkit_chunks::{BlockKind, BlockPos, World};\nuse rustbukkit_protocol::encode_block_update;\n"

# Fix the enum closing - remove the misplaced imports inside the enum
$content = $content -replace "use rustbukkit_chunks::\{BlockKind, BlockPos, World\};`nuse rustbukkit_protocol::encode_block_update;`n}", "}"

[System.IO.File]::WriteAllText('crates/rustbukkit_bin/src/session.rs', $content)
Write-Host 'Fixed imports completely'
