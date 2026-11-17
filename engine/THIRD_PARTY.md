# Third-party references and reuse

Inspected on 2026-11-11. The clones were read-only reference data outside this repository.
No cloned code was built, imported, or executed. No submodules were initialized.
No third-party source code was copied or translated into this engine.

## Cloned references

The license classification below comes from reading the actual checked-out license text, not GitHub metadata.
Each listed MIT file contains the permission grant, notice condition, and warranty disclaimer.

| Repository | Exact inspected commit | Checked license and copyright | Scope |
|---|---|---|---|
| [FerrisOfficial/CatanAPI](https://github.com/FerrisOfficial/CatanAPI) | `eed03e01db09d48dc7605fbf7049e053f065486f` | [LICENSE](https://github.com/FerrisOfficial/CatanAPI/blob/eed03e01db09d48dc7605fbf7049e053f065486f/LICENSE), MIT, copyright 2026 Maciej Stempniak | Packed state, action generation, apply/undo, and longest-road source inspected. No reuse. |
| [Eli6th/catan-rl](https://github.com/Eli6th/catan-rl) | `021279c56834b6203480e5292e1de7246e47bd68` | [LICENSE](https://github.com/Eli6th/catan-rl/blob/021279c56834b6203480e5292e1de7246e47bd68/LICENSE), MIT, copyright 2026 Eli Olcott | Rust state, building rules, cache updates, and performance report inspected. No reuse. |
| [nogulong/rust-catan-rl](https://github.com/nogulong/rust-catan-rl) | `f734fc74edb707c52f3d59669b54145e274a287d` | [LICENSE](https://github.com/nogulong/rust-catan-rl/blob/f734fc74edb707c52f3d59669b54145e274a287d/LICENSE), MIT, copyright 2026 nogulong. [LICENSE.rust_catan](https://github.com/nogulong/rust-catan-rl/blob/f734fc74edb707c52f3d59669b54145e274a287d/LICENSE.rust_catan), MIT, copyright 2020 Swynfel. | Core state/API and transition source inspected. JSettlers is excluded. No reuse. |
| [Swynfel/rust-catan](https://github.com/Swynfel/rust-catan) | `499632e4073ae5a03f0d404675c955fc8911c312` | [LICENSE](https://github.com/Swynfel/rust-catan/blob/499632e4073ae5a03f0d404675c955fc8911c312/LICENSE), MIT, copyright 2020 Swynfel | State/API and transition source inspected. No reuse. |

The nogulong repository declares `external/jsettlers` as a submodule and identifies it as GPL in its README.
Its MIT license does not cover that component. The submodule was not initialized or inspected.

## Other primary sources

[Stockfish](https://github.com/official-stockfish/Stockfish) was read through its public source URLs.
The observed HEAD was `def601cc156f7d41f9ddea76d74515eddf232aaf`.
The inspected `position.h`, `position.cpp`, and `perft.h` headers state GPL version 3 or later.
Only general state-restoration and tree-counting concepts informed the notes. No Stockfish code was copied or translated.

Current Rust compiler, Cargo, language-reference, and standard-library documentation supplied compiler-setting and SIMD facts.
No documentation code examples were copied.

The earlier audit mentions GPL and unlicensed engines. This research did not clone or copy those engines.
Their reported behavior remains secondary evidence from that audit.

## Development tools

Profiling used the official Arch packages `perf` 7.3.1-1 and `libpfm` 4.14.1-1. Their detached signatures were verified with the Arch keyring before extraction.
The benchmark machine used official rustup/Rust binaries.
These executable tools are not engine source or committed dependencies. No code was copied from them.

## Reuse ledger

Current reused source files or snippets: **none**.

Before future MIT reuse, record the upstream commit, original path, destination path, and modifications here.
Include the complete applicable copyright and MIT permission notice with copied or substantial portions.
For code inherited through nogulong from Swynfel, preserve both applicable notices.
Do not copy GPL, unlicensed, or separately licensed component code under a repository-level MIT assumption.
