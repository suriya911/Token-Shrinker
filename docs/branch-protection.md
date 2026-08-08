# Branch protection baseline

Apply these settings to `main` in the GitHub repository before accepting external contributions:

- require a pull request with one approval;
- dismiss stale approvals when new commits are pushed;
- require review from CODEOWNERS for owned paths;
- require the `Check (ubuntu-latest)`, `Check (macos-latest)`, and `Check (windows-latest)` status checks;
- require conversations to be resolved;
- block force pushes and branch deletion;
- enable private vulnerability reporting;
- keep GitHub Actions referenced by immutable commit SHA.

Signed commits are recommended once every maintainer has signing configured. Do not make them mandatory before that migration is complete.

These repository-hosted settings are not changed by the source tree. A repository administrator must enable them in GitHub after the first CI run establishes the check names.
