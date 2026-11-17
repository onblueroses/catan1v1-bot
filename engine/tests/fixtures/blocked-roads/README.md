# Captured blocked-road counterexamples

These eleven valid snapshots came from the paused native `7b0b339` balanced-dice, seed-41 cohort. Each board matched exactly once in its immutable 65,536-element prepared input array. A known index-zero board was the positive control. No game was rerun to recover the indices.

The process stayed stopped and resumable during read-only capture. The native compiler was Rust 1.99.1. Safe field-offset mapping rebuilt canonical bytes before decoding. Every captured cache field matched independent full reconstruction. The executable SHA-256 was `aacb6235b775a31cd533e4134e7bc85da91b686dbbd4ab2c4aa06e2d805d5a5e`.

Each `.hex` contains exactly 313 canonical bytes. Each `.jsonl` is a zero-step snapshot trace under the corrected E10 rule. It is a snapshot witness, not a constructor reachability trace. The separate origin generator produces full constructor traces from exact `prepare(41,index)`, stopping at the first corrected certificate without a length limit. The captured billion-turn snapshots remain separate evidence.

| Origin index | Snapshot hash | Turn | Points | Road pieces left |
|---:|---|---:|---|---|
| 9116 | `0a90558550ad22c7` | 675896413 | [13, 13] | [0, 6] |
| 10842 | `d8c057c2ffe00dc9` | 912630126 | [14, 14] | [0, 3] |
| 16875 | `0c82494c4d35564b` | 907162436 | [14, 14] | [0, 7] |
| 17249 | `25802412ac6f9095` | 630066216 | [13, 11] | [0, 3] |
| 18287 | `843b77884e142c57` | 777125855 | [12, 14] | [1, 0] |
| 40740 | `a9f1431a136bf4ec` | 896539105 | [8, 14] | [10, 0] |
| 46545 | `2240b8f5f20f7c5a` | 845566123 | [9, 14] | [10, 0] |
| 50386 | `3c73151b339f0bf8` | 737242817 | [13, 14] | [3, 0] |
| 53216 | `6377fcc1cce7e44e` | 568601172 | [14, 13] | [1, 0] |
| 57787 | `965e934cf9aa62d5` | 567365500 | [12, 13] | [0, 2] |
| 58986 | `15c41219edbaff81` | 1032742545 | [12, 11] | [7, 0] |

In every snapshot, each player either has no road piece or has no geometrically eligible road edge. No settlement site or available city upgrade remains. The development deck is empty and every knight is already spent. Thus road/building occupancy, both awards and held VP cards cannot change. Additional reachable points are zero for both players.

Raw-edge checks ignore affordability and reject passage through opponent buildings. The full argument is in `diagnostics/ENCODING-blocked-roads.patch`. Tests contain synthetic controls that open a formerly blocked connection while leaving all other certificate conditions true. They reject dead status even with no wood or brick.

The raw stack and prepared-array captures, their SHA-256 manifests, native offsets and raw geometry reports are not part of this repository. No entry of the original 418-file trace corpus was changed.
