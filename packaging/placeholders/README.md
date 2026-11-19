# Name placeholders

Minimal packages that reserve the names `karez`, `karez-chunk`, `karez-proto-pg`, `karez-pg` and `karez-rt` on crates.io and
`karez` on npm and PyPI (owner's decision of 2026-11-13). They contain no code. The real crates are in `crates/` and are
not published (`publish = false`). Publish order does not matter. Nothing here is a release.

Published on crates.io on 2026-11-16: `karez`, `karez-chunk`, `karez-proto-pg`, `karez-pg`, `karez-rt` (0.0.1).
Not yet published: `karez` on npm (`cd npm/karez && npm publish --access public` after `npm login`) and on PyPI (`cd pypi/karez && python -m build && twine upload dist/*` with a PyPI token).
