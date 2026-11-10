foreach ($f in (Get-ChildItem "C:\Users\Conner Adams\.codex" -File -Recurse -Include *.sqlite,*.sqlite3,*.db -ErrorAction SilentlyContinue) {
  try {
    $c = New-Object System.Data.SQLite.SQLiteConnection("Data Source=$($f.FullName);Version=3;ReadOnly=1")
    $c.Open()
    $cmd = $c.CreateCommand()
    $cmd.CommandText = "SELECT name FROM sqlite_master WHERE type='table'"
    $dr = $cmd.ExecuteReader()
    $tables = while ($dr.Read()) { $dr['name'] }
    $dr.Close()
    Write-Host "=== $($f.Name) tables: $($tables -join ', ') ==="
    foreach ($t in $tables) {
      if ($t -match "goal|record|task|todo|note|memory|thread|session") {
        $cmd2 = $c.CreateCommand()
        $cmd2.CommandText = "SELECT * FROM $t LIMIT 3"
        $dr2 = $cmd2.ExecuteReader()
        while ($dr2.Read()) {
          $row = @{}
          for ($i=0;$i -lt $dr2.FieldCount;$i++) { $row[$dr2.GetName($i)] = $dr2.GetValue($i) }
          Write-Host ("  $t : " + (($row.Keys | For-Object {"$($_)=$($row[$_)"}) -join " | "))
        }
        $dr2.Close()
      }
    }
    $c.Close()
  } catch { Write-Host "ERR $($f.Name): $($_.Exception.Message)" }
}
