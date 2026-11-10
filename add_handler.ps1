$content = Get-Content "crates/rustbukkit_bin/src/session.rs" -Raw
$handler = Get-Content "handler.txt" -Raw
$content = $content -replace "fn offline_uuid\(username: &str\) -> Uuid \{", $handler + "fn offline_uuid(username: &str) -> Uuid {"
[System.IO.File]::WriteAllText("crates/rustbukkit_bin/src/session.rs", $content)
Write-Host "Handler added"
