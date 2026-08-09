# Token-Shrinker for VS Code

This thin extension connects to the local Token-Shrinker binary through its versioned SDK. It can
show health, build bounded repository context, and display content-free statistics.

Repository reads and statistics are disabled in untrusted workspaces. Status checks remain
available because they do not send workspace content. The configured binary is launched directly,
without a shell, and the extension never changes model-provider endpoints.
