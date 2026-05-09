Output demo/grep.gif

Set Shell "bash"
Set FontSize 14
Set Width 900
Set Height 500
Set Theme "Dracula"
Set Padding 20
Set TypingSpeed 50ms

Type "# Hybrid search: BM25 + semantic embeddings + graph proximity"
Sleep 500ms
Enter
Sleep 300ms

Type `git-semantic grep "how embeddings are generated and cached"`
Sleep 500ms
Enter
Sleep 3s

Type "# Exact identifier lookup — BM25 finds it precisely"
Sleep 500ms
Enter
Sleep 300ms

Type `git-semantic grep "search_hybrid" -n 5`
Sleep 500ms
Enter
Sleep 3s
