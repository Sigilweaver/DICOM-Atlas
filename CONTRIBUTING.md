# Contributing

Thanks for your interest in DICOM-Atlas. The project welcomes
issues, pull requests, vendor data contributions, and bug reports.

## Ways to contribute

- **Tag corrections** - if a private tag's name, VR, or VM
  disagrees with a vendor conformance statement you have, open an
  issue with a citation (PDF title, page, table) and we will
  verify and patch the data.
- **New vendor coverage** - if you have access to conformance
  documents for a vendor we do not yet cover, open an issue first
  so we can coordinate the scrape (the corpus needs to be hosted
  on `archive.org` for reproducibility).
- **Code** - bug fixes, performance improvements, and new query
  ergonomics in `dicom-map`, `dmap-compiler`, the C FFI, or the
  Python bindings.
- **Docs** - README, Docusaurus site, or doc-comment improvements.

## Development setup

```sh
# Rust
cargo build --workspace
cargo test --workspace

# Python bindings (editable)
cd dicom-map-py
maturin develop --release
pytest ../tests/

# Compile a fresh tags.dmap from checked-in JSONL snapshots
cargo run --release --bin dmap-compile -- \
    --standard data/standard/attributes.json \
    --resolved data/resolved_pydicom_backfilled.jsonl \
    --out tags.dmap
```

See [DEVELOPMENT.md](DEVELOPMENT.md) for the scraper pipeline and
corpus maintenance workflow.

## Pull request checklist

- `cargo fmt --all` clean.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo test --workspace` passes.
- `pytest tests/` passes.
- `python tests/roundtrip_fuzz.py` passes (500 samples by
  default).
- If you change the schema or `tags.dmap` layout, bump the
  workspace version in the root `Cargo.toml` and add a note to
  [CHANGELOG.md](CHANGELOG.md) under `[Unreleased]`.
- If your change is user-facing, update the README and/or the
  Docusaurus site in `docs/`.

## Code of conduct

By participating you agree to abide by the Sigilweaver
[Code of Conduct](https://github.com/Sigilweaver/.github/blob/main/CODE_OF_CONDUCT.md).

## License

Code contributions are accepted under the project's
[Apache-2.0 License](LICENSE). Data contributions (private tag
entries, conformance citations) are accepted under the
[CC-BY-SA-4.0 License](LICENSE-DATA).

By submitting a contribution you certify that you have the right
to submit the work under the relevant license and that you agree
to the project's contributor sign-off (DCO):
<https://developercertificate.org/>.
