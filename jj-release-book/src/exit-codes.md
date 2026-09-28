# Exit codes

`jj-release` uses distinct exit codes so CI scripts can distinguish failure
modes:

| Code | Meaning                                      |
| ---- | -------------------------------------------- |
| 0    | Success, or no trigger found (nothing to do) |
| 2    | Missing or invalid configuration             |
| 3    | Missing required CLI tool (`gh`, `glab`)     |
| 101  | General release failure                      |

Exit code 1 is reserved for unexpected panics. All handled errors use the codes
above. The `Silent` exit state passes through an arbitrary code for cases where
the error was already reported upstream.
