## Annex A (informative) Limits

The limits set throughout this standard, gathered for implementers. Where this table and the text differ, the text prevails.

| What | Limit | Section |
|---|---|---|
| Prefix of an object name | 2 to 8 characters | 2.1 |
| Hash in an object name | exactly 40 hexadecimal digits; 64 reserved | 2.1 |
| Tag of a draft name | 1 to 40 characters | 2.4 |
| A name of a file or directory | 1 to 64 characters | 3.1 |
| Levels of directory within an object | at most 6 `/` in a relative path | 3.1 |
| A path within an object | 80 characters | 3.1 |
| Files in an object | 2,000 | 3.1 |
| One file of an object | 10 MiB | 3.1 |
| A whole object | 50 MiB | 3.1 |
| A manifest | 256 KiB | 3.2.3 |
| Nesting in a manifest | 16 levels | 3.2.3 |
| An integer in a manifest | 18 digits | 3.2.3 |
| A floating-point number written in a manifest | 32 characters | 3.2.3 |
| `lock.json` | 1 MiB | 3.3 |
| A note | 256 KiB | 4.1 |
| A source list beside an object; a removal record | 1 MiB each | 4 |
| `reason` of a removal record | 1,000 characters | 4.2 |
| Sources in a removal record | 3 | 4.2 |
| A line of a source list | 64 KiB | 5.1 |
| `label` and `license` in a source list | 256 characters each | 5.1 |
| The path of an include line | 256 characters | 5.1 |
| Levels of include | 8 | 5.1 |
| A group | 1 to 4 parts, 128 characters | 5.1 |
| A `git` address | 512 characters | 5.1 |
| A `local` path | 1,024 characters | 5.1 |
| Sources used per object, and per include line on a publication branch | 3 | 5.1 |
| A volume of a library list | 16 MiB | 5.2 |
| The path before an object directory | 60 characters | 6.2 |
| An object name | 49 characters | 6.2 |
| A path within an object, with its leading `/` | 81 characters | 6.2 |
| The path of any file in a library | 190 characters | 6.2 |
| A branch name | 100 characters | 6.3 |
| Sources tried when fetching an object | 3 | 7.3 |
