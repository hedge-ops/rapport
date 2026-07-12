# rapport-command

`rapport-command` runs external tools for Rapport. It captures command output,
supports bounded concurrent batches, and coordinates exclusive machine-local
resources across Rapport processes.

The crate deliberately does not interpret Git, Just, build stages, or Rapport
workflow concepts. Higher layers decide what a command means and when it is
eligible to run.
