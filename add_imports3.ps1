$content = Get-Content "crates/rustbukkit_bin/src/session.rs" -Raw
$content = $content -replace "use crate::world_persistence::WorldStore;`r`n", "use crate::world_persistence::WorldStore;`r`nuse rustbukkit_chunks::{BlockKind, BlockPos, World};`r`nuse rustbukkit_protocol::encode_block_update;`r`n"
[System.IO.File]::WriteAllText("crates/rustbukkit_bin/src/session.rs", $content)
Write-Host "Imports added"
