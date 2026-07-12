# rapport-git

`rapport-git` provides concrete Git repository operations for Rapport. It owns
Git concepts such as repositories, revisions, status, merge bases, and
source-side changed files while delegating process execution to
`rapport-command`.

It does not know about GitHub, Rapport Work, build stages, Reviews, or shipping.
