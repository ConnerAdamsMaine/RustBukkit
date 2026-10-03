note: 70 or so lines before we slow down significantly

---

## Original impl. used this

min_x >= neg_bound && min_z >= neg_bound && max_x < pos_bound && max_z < pos_bound

2026-01-06T01:04:22.644227Z INFO 145: [STARTUP] Pregeneration complete: 256 new chunks in 27.46s (9 chunks/sec), cache: 256/1129

---

let x_fits = min_x >= neg_bound && max_x < pos_bound;
let z_fits = min_z >= neg_bound && max_z < pos_bound;
x_fits && z_fits

2026-01-06T00:47:53.201839Z INFO 279: [CHUNK] Flushed 256 chunks to disk in 27.60s (9 chunks/sec)

---

[min_x, min_z].iter().all(|&min| min >= neg_bound)
&& [max_x, max_z].iter().all(|&max| max < pos_bound)

2026-01-06T00:49:47.212166Z INFO 279: [CHUNK] Flushed 256 chunks to disk in 28.07s (9 chunks/sec)

---

let min_corner = [min_x, min_z];
let max_corner = [max_x, max_z];
min_corner.iter().all(|&min_c| min_c >= neg_bound)
&& max_corner.iter().all(|&max_c| max_c < pos_bound)

2026-01-06T00:51:28.015418Z INFO 279: [CHUNK] Flushed 256 chunks to disk in 27.19s (9 chunks/sec)

---

##### Before

2026-01-06T02:49:07.974633Z INFO 102: [STARTUP] Pregenerating spawn area (16x16 chunks)...
2026-01-06T02:49:08.258571Z WARN 346: [CHUNK - V1] Flushing all cached chunks to disk...
.... chunk gen logs
Saved Chunk ...
2026-01-06T02:49:36.401576Z INFO 371: [CHUNK] Flushed 256 chunks to disk in 28.07s (9 chunks/sec)
2026-01-06T02:49:36.401585Z INFO 149: [STARTUP] Pregeneration complete: 256 new chunks in 28.43s (9 chunks/sec), cache: 256/1129

##### Before~

2026-01-06T03:32:51.477083Z WARN 359: [CHUNK - V1] Flushing all cached chunks to disk...
.... chunk gen logs
Saved Chunk ...
2026-01-06T03:33:19.193351Z INFO 385: [CHUNK] Flushed 256 chunks to disk in 27.65s (9 chunks/sec)
2026-01-06T03:33:19.193361Z INFO 162: [STARTUP] Pregeneration complete: 256 new chunks in 27.97s (9 chunks/sec), cache: 256/1129

##### After

2026-01-06T03:33:58.739813Z WARN 273: [CHUNK - V2] Flushing all cached chunks to disk...
2026-01-06T03:33:59.249269Z DEBUG 325: Saved 64 chunks to region file "/home/dwarf/Documents/GitHub*Projects/Rust/OTHER/RustBukkit-Server/world/region*-32*-32*-1*-1.dat"
2026-01-06T03:33:59.249414Z DEBUG 325: Saved 64 chunks to region file "/home/dwarf/Documents/GitHub_Projects/Rust/OTHER/RustBukkit-Server/world/region_0*-32*31*-1.dat"
2026-01-06T03:33:59.250031Z DEBUG 325: Saved 64 chunks to region file "/home/dwarf/Documents/GitHub*Projects/Rust/OTHER/RustBukkit-Server/world/region*-32*0*-1_31.dat"
2026-01-06T03:33:59.264256Z DEBUG 325: Saved 64 chunks to region file "/home/dwarf/Documents/GitHub_Projects/Rust/OTHER/RustBukkit-Server/world/region_0_0_31_31.dat"
2026-01-06T03:33:59.264308Z INFO 339: [CHUNK] Flushed 256 chunks to disk in 0.52s (488 chunks/sec)
2026-01-06T03:33:59.274645Z INFO 162: [STARTUP] Pregeneration complete: 256 new chunks in 0.78s (327 chunks/sec), cache: 256/1129
