#!/usr/bin/env sh
set -eu

curl -sSL https://github.com/rust-lang/mdBook/releases/download/v0.5.2/mdbook-v0.5.2-x86_64-unknown-linux-gnu.tar.gz | tar -xz
curl -sSL https://github.com/badboy/mdbook-mermaid/releases/download/v0.17.0/mdbook-mermaid-v0.17.0-x86_64-unknown-linux-gnu.tar.gz | tar -xz
