# astraeadb

The command-line interface for [AstraeaDB](https://github.com/AstraeaDB/AstraeaDB-Official),
a graph database with built-in vector search, written in Rust.

```bash
cargo install astraeadb
astraeadb serve
```

This package exists so that the obvious command works. The binary has been
called `astraeadb` for some time, but the package shipping it was named
`astraea-cli`, so `cargo install astraeadb` failed with "could not find
`astraeadb` in registry".

`cargo install astraea-cli` installs the identical program and continues to
work. Both packages are thin `main` functions over the same library entry
point, so there is no difference between them beyond the name you type.

Start with the [twenty-lesson course](https://astraeadb.github.io/getting-started/),
which takes you from installing a server to training a graph neural network.
