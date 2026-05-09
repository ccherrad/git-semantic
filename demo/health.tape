Output demo/health.gif

Set Shell "bash"
Set FontSize 14
Set Width 900
Set Height 400
Set Theme "Dracula"
Set Padding 20
Set TypingSpeed 50ms

Type "# Codebase health: cohesion/coupling heatmap across all communities"
Sleep 500ms
Enter
Sleep 300ms

Type "git-semantic health"
Sleep 500ms
Enter
Sleep 2s

Type "# Drill into a specific community"
Sleep 500ms
Enter
Sleep 300ms

Type `git-semantic health --community "embeddings"`
Sleep 500ms
Enter
Sleep 2s
