# ci/

Staging area for GitHub Actions workflow changes.

This directory exists because session credentials cannot write
`.github/workflows/*` — see admin `DECISIONS.md` ADR-8. To change CI:

1. Put the intended workflow file in `workflows/`.
2. A maintainer promotes it with the admin `rollout/apply-ci-folders.sh`
   script.

## Pending

Nothing.

## Promoted

Both of these were staged here and now run from `.github/workflows/`:

- **`docs.yml`** — the prose gate: Vale over the reader-facing pages at
  the levels set in `.vale.ini`, on the file list
  `ts/scripts/gated-docs.cjs` produces. See `docs/STYLE-GUIDE.md`.

  It needs no sibling checkouts and no secrets, and pins its own Vale
  version. Errors fail the job; warnings go to the run summary as a
  report. `make prose` runs the identical check locally, and the test
  suite runs the other half of the gate (`ts/test/docs.test.js`).

- **`rust.yml`** — the Rust gate for the `rs/` crate. It runs
  `ci/rust/run.sh`, which is the same script a contributor runs locally,
  so the hosted and local gates cannot drift.

  It needs no secrets, but it does need the four sibling checkouts the
  crate resolves by path (`parser`, `json`, `jsonic` and `support`,
  cloned by the job), and network access to w3.org for the W3C
  conformance corpus, which the suite fetches on first use and which
  fails the run rather than skipping when it cannot be fetched. It is a
  standalone workflow rather than an arm of `ci.yml`, because `ci.yml`
  calls the org-shared polyglot workflow and that takes no Rust input.
