$content = Get-Content 'crates/rustbukkit_bin/src/session.rs' -Raw
# Remove the misplaced imports
$content = $content -replace "use rustbukkit_chunks::\{BlockKind, BlockPos, World\};`nuse rustbukkit_protocol::encode_block_update;`n}", "}"
# Add the imports after the crate imports
$content = $content -replace "use crate::world_persistence::WorldStore;`n", "use crate::world_persistence::WorldStore;`nuse rustbukkit_chunks::{BlockKind, BlockPos, World};\nuse rustbukkit_protocol::encode_block_update;\n"
[System.IO.File]::WriteAllText('crates/rustbukkit_bin/src/session.rs', $content)
Write-Host 'Fixed imports'
