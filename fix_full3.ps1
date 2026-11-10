$content = Get-Content 'crates/rustbukkit_bin/src/session.rs' -Raw

# Fix the enum closing - remove the misplaced imports inside the enum
$content = $content.Replace("use rustbukkit_chunks::{BlockKind, BlockPos, World};
use rustbukkit_protocol::encode_block_update;
}", "}")

# Ensure the imports are in the right place (after world_persistence)
if ($content -notmatch "use rustbukkit_chunks::\{BlockKind, BlockPos, World\};") {
    $content = $content -replace "use crate::world_persistence::WorldStore;\n", "use crate::world_persistence::WorldStore;\nuse rustbukkit_chunks::{BlockKind, BlockPos, World};\nuse rustbukkit_protocol::encode_block_update;\n"
}

[System.IO.File]::WriteAllText('crates/rustbukkit_bin/src/session.rs', $content)
Write-Host "Fixed imports completely"
