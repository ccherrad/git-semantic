Output demo/navigate.gif

Set Shell "bash"
Set FontSize 14
Set Width 900
Set Height 550
Set Theme "Dracula"
Set Padding 20
Set TypingSpeed 50ms

Type "# Step 1 — orient: find the subsystem for a task"
Sleep 500ms
Enter
Sleep 300ms

Type `git-semantic map "where does embedding dispatch happen"`
Sleep 500ms
Enter
Sleep 3s

Type "# Step 2 — read cheaply: outline = names + line ranges (~96% token reduction)"
Sleep 500ms
Enter
Sleep 300ms

Type "git-semantic get src/embed.rs --mode outline"
Sleep 500ms
Enter
Sleep 2s

Type "# Step 3 — retrieve exactly: pull only the chunk you need"
Sleep 500ms
Enter
Sleep 300ms

Type "git-semantic get src/embed.rs:9-17"
Sleep 500ms
Enter
Sleep 2s
