# rss-builder

Builds RSS feed from a directory of HTML files

## Installation
1. Install Rust: https://rust-lang.org/tools/install/
2. Run `cargo install --path .`

## Running
Suppose you have a directory named `posts` with HTML files that you want to build an RSS feed named "Rodion's Blog" from:
```
rss-builder --dir=posts --base-url=https://rodio.codeberg.page/ --title="Rodion's Blog"
```

Publication dates of existing articles are persisted across runs.

Other options: `rss-builder --help`
